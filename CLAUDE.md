# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`jev-desk`: a Rust workspace demonstrating TypeSafe's Jev model as a **typed judgment layer** over deterministic trading/optimisation code. Two domains (spot forex, battery energy trading) share six primitives in `jev-core`. Synthetic data only. The one-line thesis, which every design decision follows from: *the solver owns the numbers, Jev owns the judgment, the schema owns the contract.*

`README.md` is thorough and current — read it for the primitive catalogue, the results tables, and the API surface before changing anything user-facing.

## Commands

`just` is the entry point (`just` alone lists recipes). `set dotenv-load` is on, so a `.env` with `TYPESAFE_API_KEY` is picked up.

```sh
just test                 # cargo test --workspace; offline by construction, no key needed
just check                # fmt --check + clippy -D warnings + test  (run before committing)
just check-all            # the above plus the UI typecheck (needs node)
just fmt

just demo-fx-mock         # forex run + CPI-day replay, offline
just demo-battery-mock    # battery run + grid-notice replay, offline
just demo-all-mock        # both + one Explain for Compliance
just prompts / just schemas   # print standing instructions / JSON Schemas

just desk                 # ui-build then serve on :8787
just serve                # API only, mock default; serves ui/dist if built
just serve-live           # API defaulting to System One; needs TYPESAFE_API_KEY
just ui-dev               # Vite on :5173 proxying /api -> :8787 (run `just serve` too)
```

Single test / single crate:

```sh
cargo test -p fx --test honesty                      # one integration-test binary
cargo test -p jev-core --test primitives gate_       # tests matching a substring
cargo test -p server --test api
```

Drop `-mock` from a demo recipe to hit the live API; `--mock` is the *only* switch, so every code path is otherwise identical. CLI flags (`--seed`, `--days`, `--limit N`, `--out`) are global and pass through `just demo-* -- --flag`.

Output lands in `out/<domain>/report.md` and `out/<domain>/decisions.jsonl` (gitignored).

## Architecture

### Crate graph

```
synth ──► fx ──────┐
       └► battery ─┼──► report ──► cli      (jev-desk binary)
jev-core ──────────┘          └──► server   (jev-desk-server binary, axum)
                                             ▲
ui/ (Vite + React + TS, no UI lib) ──────────┘  mirrors server/src/dto.rs
```

- **`jev-core`** — the six primitives (`Gate`, `Classify`, `Score`, `Check`, `Rank`, `Explain`), the `JevClient` transport trait with two impls (`LiveJev` over `typesafe-ai-sdk`, `MockJev` rule-based), `Pipeline` with its `Batch` fan-out, `ReviewPolicy`, and `Audit`. Domain-agnostic.
- **`synth`** — seeded generators for both worlds. Pure functions of a `u64` seed with hand-rolled PRNG and date arithmetic so a seed pins a world across platforms and dependency updates. Separate RNG streams per concern.
- **`fx`, `battery`** — each has `strategy.rs`/`solver.rs` (the numbers), `features.rs` (the *only* thing Jev reads), `judgment.rs` (the pipeline of primitives), `engine.rs` (fills/execution and P&L). `run_session` judges every candidate/day and produces an ungated and a gated book.
- **`report`** — terminal + Markdown rendering and the `Explain` stage. `tests/headline_claims.rs` asserts the exact figures quoted in `README.md`.
- **`cli`** — the demos and the counterfactual replays (`replay.rs`).
- **`server`** — the same runs as a JSON API. Computes nothing new: `run.rs` is the CLI minus printing, `store.rs` keeps runs in memory (`RunHandle` with a growing `Audit` for progress polling), `project.rs` maps domain types onto `dto.rs` wire types.

### How a Jev call works

Every primitive goes through `Jev::call` (`jev-core/src/jev.rs`), which is the single point where tokens, latency and the question/answer pair are recorded. A call is **state + asks, nothing else**, and it follows the System One docs' split:

- the **state** is content only, shaped `{"context": …, "input": …, "prior_judgments": […]}`. `context` is `JevInput::context_block()`: a sentence or two of domain framing (which solver proposed this; its numbers are final). Keep it to facts the input's fields don't already carry — no restated numbers, no instructions. It is sent to the API verbatim.
- the **asks**: the System One API answers only three question kinds — `Ask::Noul` (P(yes)), `Ask::Choice` (label + full distribution), `Ask::Score` (weighted rubric position). It does not emit free text. Every ask's `instructions` is a JSON **object**: `{"question": <caller's text>}` merged with the primitive's standing guidance from `crates/jev-core/prompts/<primitive>.json` (named fields — `what`, `not_for`, `evidence` — one object per question part; `prompts::instructions()` does the merge). There is no prose prompt anywhere, and nothing instruction-like may go into the state (`jev-core/tests/pipeline.rs` asserts this).

Consequently the *judgment* fields of every output (action, label, score, check booleans, rank order) come from Jev, while every `reason`/`note`/`rationale`/`summary` string is **composed deterministically in `jev-core`** from those verdicts plus the caller's vocabulary. Don't add a free-text channel; a reason must not be able to assert anything the schema didn't carry.

**Known rules stay in code.** Jev is not a calculator: a check that is a threshold comparison (`stop_sane`, `reserve_ok`, `cycle_budget_ok`) is decided by the domain via `CheckSpec::rule(...)`, reported with `source: Rule` and `p` of exactly 0/1, and its outcome is published as a plain flag in the features (`stop_clears_noise`, `reserve_breached`, `cycles_within_budget`) for the model and the mock to read. Only judgments (`signal_valid_in_regime`, `correlated_exposure_ok`, `margin_plausible`) go to Jev. `Rank` is one fit `Score` per candidate on a shared rubric, sorted in code — never a single Choice read as an ordering.

`validate()` on each output is a hard error path (range, length, enum membership, contradictions like `hold` with size). Nothing degrades to `Action::Execute`.

### Pipeline composition

`Pipeline::new(jev, "domain")` accumulates priors; each stage's typed output is appended to `prior_judgments` in the state of every later stage (`Staged` overrides `to_state`) — once, as JSON with a one-line `line` gist; it is not also rendered into the framing. Independent stages are fanned out with `pipeline.batch(stage, input)` → `.classify/.check/.score/.rank/.gate/.explain(...)` returning typed `Slot`s → `.send().await` → `out.take(slot)`. A batch of more than one is recorded as one `Primitive::Batch` call whose `output` lists `{stage, primitive, output}` per stage, with asks named `<stage>.<ask>`; a batch of one is recorded under its own primitive with bare ask names (the single-stage `Pipeline` methods are batches of one). Forex is `[Classify, Check, Score] → Gate` (2 calls); battery is `[Classify, Rank] → [Check, Score] → Gate` (3 calls; the second batch's state is rebuilt around the winning schedule). After the gate, the domain's `ReviewPolicy` (`REVIEW` const in each `judgment.rs`) sets `judgment.review` in code from the classify confidence and undecided checks; it never changes the action.

**One decision, several calls.** `Pipeline::new(jev, "fx").judging(id)` tags every call the pipeline makes with the decision it is judging, and `CallRecord::decision` carries it into `decisions.jsonl` and the API. The ids come from `fx::decision_id(index)` and `battery::decision_id(day)` — the same index the API's detail endpoints take — and the decision rows echo them under `decision_id`, so the two (fx) or three (battery) calls behind a trade can be totalled back onto it. Replays judge the same candidate in a changed world, so they pass their own ids (`fx:replay:with-event`, `battery:replay:quiet`) rather than colliding with the session's; the report's run-level `Explain` calls stay untagged.

### Cost

`jev-core/src/cost.rs` prices the audit log: `Audit::ledger(Rates)` returns a `CostLedger` with the run's total and each decision's share, and `Rates::from_env()` reads `JEV_USD_PER_MTOK_IN` / `JEV_USD_PER_MTOK_OUT`, falling back to `Rates::ASSUMED` ($3/$15 per Mtok). Calls, tokens and latency are measured; **the dollar figure is an estimate and is always rendered as one** — a mock run bills nothing, and System One's price list is not in this repo. Anything printing it says `assumed` when the stand-in rates were used (`report::cost` for both report footers, the `cost` block and per-decision columns over HTTP). Keep that labelling if you touch it.

### The honesty invariant

The forex generator has a latent regime that drives prices. **Nothing in the judgment path may read it.** It lives in `PairSeries::truth`, parallel to the bars, never on a `Bar`; `FxFeatures::compute` takes `&[Bar]`, not the series; `true_regime` is attached to a `DecisionRecord` only *after* judgment. `crates/fx/tests/honesty.rs` asserts no audited state contains it and that the classifier is therefore *not* perfect. The server test asserts the same over HTTP (regime in the `outcome` row, never in `features`). When adding a feature or a DTO field, keep this in mind — a leak silently invalidates the gated-vs-ungated comparison.

`MockJev` respects the same rule: it reads only the serialised state, and where the spec names the true regime it uses an observable stand-in (`trend_strength`, Kaufman efficiency ratio). All mock rules and thresholds are in `crates/jev-core/src/mock.rs`; action and size come from one rule evaluation so they can't contradict. Mock rules key on the bare ask name (they strip a `<stage>.` batch prefix), and the score's "any check fails" is derived from features because in a batch the check is not yet a prior. The `informative` flag on generated headlines is generator truth too — send the headline text, never a count of the flagged ones.

### Server ↔ UI contract

`crates/server/src/dto.rs` and `ui/src/api/types.ts` are maintained by hand, field for field — nothing generates one from the other. `crates/server/tests/api.rs` asserts the exact key lists, so renaming a DTO field means updating `types.ts` and the test together. The router is exercised directly via `tower::ServiceExt::oneshot`, no listener.

Backend choice is per request (`mock` in the `POST /api/runs` body); `--live` only changes the default. A live request without `TYPESAFE_API_KEY` is refused with a reason.

## Conventions worth knowing

- Tests never touch the live API. `jev-core/tests/live_client.rs` covers the translation layer against a `wiremock` stub. Shared fixtures (`TestInput`, `Recorder`) live in `jev-core/tests/common/mod.rs`.
- `report/tests/headline_claims.rs` pins the README's numbers to the default seed (including the review counts). If a change moves them, update the README *and* the test together rather than loosening the assertion.
- All library crates are `#![warn(missing_docs)]` and `just check` runs clippy with `-D warnings`; doc every public item.
- `rustfmt.toml`: `max_width = 100`, `use_small_heuristics = "Max"`.
- `jev-core/src/ask.rs` deliberately mirrors the SDK's types rather than re-exporting them (the SDK's answer structs are `#[non_exhaustive]`, so the mock couldn't construct them).
- A missing API key exits with status 2 and a hint, not a backtrace — keep that behaviour in any new entry point.
