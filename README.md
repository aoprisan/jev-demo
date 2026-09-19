# jev-desk

**The solver owns the numbers, Jev owns the judgment, the schema owns the contract.**

A Rust workspace demonstrating [TypeSafe](https://typesafe.ai)'s Jev model as a
typed judgment layer over deterministic trading and optimisation code. Two
domains — spot forex and battery energy trading — share one set of reusable
primitives. Synthetic data only; no brokers, no market connections, nothing that
touches a real venue.

```sh
just demo-fx-mock        # forex, offline
just demo-battery-mock   # battery, offline
just demo-all-mock       # both, then one summary for Compliance
just test                # the whole suite, offline by construction
```

Drop the `-mock` for the live [System One
API](https://github.com/aoprisan/typesafe-ai-rust-sdk) (`TYPESAFE_API_KEY`).
`--mock` is the only switch: the same primitives, the same pipelines, the same
reports, a different backend.

---

## The idea

The deterministic side of a trading system owns the numbers — prices, sizes,
stops, schedules, P&L. It is testable, reproducible and auditable, and it should
stay that way.

What it cannot do is judge. Whether a mean-reversion signal is worth taking in
*this* regime, whether a stop is sane given how far this market actually moves,
whether today's grid notice makes the aggressive schedule a bad idea — those are
judgments, and they have historically lived in a trader's head or in a thicket
of hand-tuned thresholds.

This workspace puts them in a third place: a typed call with a schema on both
ends. Jev never produces a quantity the solver owns. It classifies, scores,
gates, checks, ranks or explains — and the result is a struct, validated before
it reaches a caller.

---

## The primitive catalogue

Six primitives, in `jev-core`. Each is a generic typed call: a domain input
struct goes in, a fixed output struct comes out, and the output is validated
before it is returned.

| Primitive | Output | Jev decides | The caller supplies |
|---|---|---|---|
| `Gate<I>` | `{ action, size_factor, reason }` | act / reduce / hold / escalate, and how much size | the actions' meanings, a size rubric |
| `Classify<I,E>` | `{ label: E, confidence, reason }` | which label of a domain enum | the enum and its descriptions |
| `Score<I>` | `{ score: 0..=100, drivers, reason }` | a position on a rubric, and which drivers hold | the rubric and the driver vocabulary |
| `Check<I>` | `{ checks: [{ name, ok, note }] }` | each named plausibility claim | the claims |
| `Rank<I,T>` | `{ ordered: [{ id, rationale }] }` | the ordering of solver-produced candidates | the candidates |
| `Explain<I>` | `{ summary, for_audience }` | framing, severity, which facts belong | the prose for each |

Each primitive's standing instructions live in
`crates/jev-core/prompts/<primitive>.md` and are identical across domains —
`just prompts` prints them. Domain framing is injected as a context block, never
as a separate prompt.

### What Jev actually returns, and what that means for `reason`

**This is the one place the design departs from a naive reading of the brief, so
it is worth stating plainly.**

The System One API answers exactly three kinds of question: a **noul**
(probability of yes), a **choice** (one label from a set you defined, plus the
whole distribution), and a **score** (a probability-weighted position on a
rubric you defined). It does not emit free text.

So the *judgment* fields of every output above come from Jev — the action, the
label, the score, each check's boolean, the ranking order. The `reason`, `note`,
`rationale` and `summary` strings are composed deterministically by `jev-core`
from those typed verdicts plus the caller's own vocabulary.

That is a stronger arrangement than free text would be, not a weaker one:

- A reason **cannot assert anything the schema did not already carry**. The
  prose is a rendering of the decision, not a second, unchecked channel of it.
- `Score`'s `drivers` are a fixed vocabulary of nouls, so every reported driver
  carries its own probability and can be tested — rather than three phrases a
  model invented.
- `Rank` reads its entire ordering off **one** choice distribution. One call, N
  candidates, and the margins come out with it.
- `Explain` chooses the framing, the severity and which facts belong; the
  wording of each is the caller's. The same run is genuinely a different summary
  for a trader and for compliance, and neither can contain a fact the run did
  not produce.

### Hard errors, never a silent fallback

A schema violation is an error, not a degraded answer. `jev-core` rejects:

- a value outside its range (`size_factor` beyond `0..=1`, `score` beyond `0..=100`);
- a string or list beyond its bound (`reason` over 160 characters, more than three drivers);
- a label outside the enum that was offered;
- an answer of the wrong kind, or a missing answer;
- a ranking that does not cover exactly the candidates it was given;
- a **contradiction** — a `hold` carrying size, or a `reduce` carrying none.

Nothing degrades to `Execute`. `just schemas` prints the JSON Schema of each
output; `validate()` enforces the same bounds in code.

---

## Composition

`Pipeline` threads each stage's typed output into the state *and* the context
block of every later stage, so a gate can see that a check failed without the
domain restating it. Every call is tagged with its stage and recorded.

**Forex** — `Classify(regime) → Check(sanity) → Score(event risk) → Gate`

```
Classify<FxRegime>        ranging | trending | event_driven
Check                     stop_sane, signal_valid_in_regime, correlated_exposure_ok
Score                     event risk 0..=100, drivers from a fixed vocabulary
Gate                      on the candidate, seeing all three above
```

**Battery** — `Classify(regime) → Rank(schedules) → Check(sanity) → Score(risk) → Gate`

```
Classify<MarketRegime>    normal | volatile | illiquid | stressed
Rank<ScheduleKind>        aggressive | balanced | reserve_heavy — all three valid
Check                     reserve_ok, margin_plausible, cycle_budget_ok
Score                     operational risk 0..=100
Gate                      on the schedule the ranking chose
```

The battery pipeline is the one that shows `Rank` doing real work: the solver
emits three complete, valid schedules and Jev orders them, after which the
checks and the gate judge the winner rather than the default.

---

## Honesty: what the judgment layer is allowed to see

The forex generator has a latent regime — ranging, trending or event-driven —
that drives its price process. **Nothing in the judgment path can read it.**

- It lives in an array *parallel* to the bars (`PairSeries::truth`), never on a
  `Bar`, so passing a bar around cannot leak it.
- `FxFeatures::compute` takes `&[Bar]`, not the `PairSeries` that holds the
  truth, so the type system keeps it out of scope.
- `MockJev` sees only the serialised state — the same thing a live model sees —
  and reads observable features only.
- `crates/fx/tests/honesty.rs` asserts that no state, context block or audit
  record ever contains it, and that the classifier therefore **does not** match
  it perfectly. A perfect score would mean the truth had leaked.

Where the brief specifies a mock rule against the true regime ("signal invalid
when true regime is Trending"), the mock uses an observable stand-in — a
Kaufman efficiency ratio over the price window. Its agreement with ground truth
is an empirical result, reported in every run, not a guarantee. The battery
world has no latent state to hide, but the same discipline applies: nothing
reads a price the desk would not yet have seen.

---

## What the demos show

### `just demo-fx-mock`

A full 90-day, three-pair run, then **one CPI day replayed twice** — once with
the release on the calendar, once with it removed. The prices are byte-identical
across both runs, so the difference is the judgment layer reacting to the event
and nothing else:

```
                  CPI scheduled  release removed
  hours to event    CPI in 0.0h   none scheduled
  event risk             92/100           62/100
  action                   hold           reduce
  size factor                0%              25%
```

### `just demo-battery-mock`

A full 90-day run, then **one day replayed with a grid notice added** over the
hours the chosen schedule discharges in. Same prices, same three schedules, one
note:

```
  rank                no notice             notice added
  1          1. balanced (0.50)  1. reserve_heavy (0.62)
  2        2. aggressive (0.36)       2. balanced (0.31)
  3     3. reserve_heavy (0.14)     3. aggressive (0.07)

  the ranking flipped: balanced -> reserve_heavy
```

### `just demo-all-mock`

Both, then a single `Explain` call summarising the whole day for Compliance.

Every run writes `out/<domain>/report.md` and `out/<domain>/decisions.jsonl` —
one JSON object per primitive call, carrying the state, the questions asked, the
verdicts returned, the typed output composed, and the tokens and latency it
cost.

---

## Results, as they actually come out

**Battery** is the clean win. The solver-only desk runs the balanced schedule
every day; the gated desk runs whatever the ranking chose:

90 days, default seed:

| | solver-only | gated |
|---|--:|--:|
| days run | 90 | 71 |
| realised margin | +11,311 | +7,155 |
| reserve payments | 87 | 812 |
| reserve breaches | **77 h on 30 days** | **0** |
| equivalent cycles | 127.6 | 71.1 |
| margin per cycle | 88.7 | 100.7 |

It gives up a third of the absolute margin, eliminates every reserve breach,
collects nine times the capacity payment, and wears the asset a little over half
as hard — earning more per cycle of wear. Standing down is not free and the
engine accounts for that: a hold forfeits the capacity payment too.

**Forex** is the interesting one, and the demo does not dress it up. The gated
book underperforms: 9.70 bp of return on notional against 16.65 ungated. The
report says why, in a section called the judgment scorecard, which holds each
named check up against the ungated outcomes it flagged:

| check | flagged | mean bp | passed | mean bp | edge |
|---|--:|--:|--:|--:|--:|
| `stop_sane` | 73 | −3.7 | 141 | +27.2 | **+30.9** |
| `signal_valid_in_regime` | 42 | +64.1 | 172 | +5.1 | **−59.1** |
| `correlated_exposure_ok` | 21 | +27.5 | 193 | +15.5 | −12.0 |

`stop_sane` earns its keep. `signal_valid_in_regime` is **inverted**: it flags
the *better* trades, and acting on it costs money.

That is not a bug in the rule, and it is worth understanding. At a band
excursion, `er ≥ 0.62` identifies the generator's trending regime with **76%
precision** against a 42% base rate — the regime call is accurate. And trades in
the true trending regime really are the worst (+8.5 bp, 37% hit rate) against
ranging (+19.1 bp, 58%). Both halves of the belief check out. What fails is the
inference: the same sharpness that marks a trend also marks an overshoot that
reverts hard, and at the band edge the second effect dominates.

The rule was left exactly as specified and the result reported, rather than
tuned until the P&L flattered the pitch. Being able to discover this at all — to
name a judgment, log it, and hold it against outcomes afterwards — is the
argument for the whole arrangement.

---

## Layout

```
crates/
  jev-core/     the six primitives, Pipeline, MockJev, the live client, prompts/, schemas
  synth/        seeded deterministic generators for both worlds
  fx/           Bollinger mean reversion, features, judgment pipeline, fill engine
  battery/      DP arbitrage solver, features, judgment pipeline, execution engine
  report/       terminal and Markdown reports, the Explain stage
  cli/          jev-desk: the three demos, the replays, prompts and schemas
```

Rust 2021, tokio, serde, schemars, clap, anyhow, and
[`typesafe-ai-sdk`](https://crates.io/crates/typesafe-ai-sdk) for the live
backend. No other runtime dependencies; the PRNG and the civil-date arithmetic
are hand-rolled so a seed pins a world exactly, across platforms and across
dependency updates.

## The synthetic worlds

**Forex** — 4h OHLC for EUR/USD, GBP/USD and USD/JPY over 90 days from a
regime-switching random walk; a monthly calendar of CPI, NFP, ECB and FOMC with
volatility aligned to it; a headline feed that is mostly noise; a starting book
of modest residual exposures.

**Battery** — hourly day-ahead prices with a daily shape, weekly seasonality,
occasional negative midday hours and evening spikes; a tick-level intraday
series that deviates from day-ahead and occasionally dislocates; a 1 MW / 2 MWh
battery with efficiency, degradation cost per cycle and SoC bounds; aFRR
commitment windows on about a third of days; grid notes and headlines.

Every generator is a pure function of a `u64` seed. Separate streams per concern,
so adding a headline does not shift the prices generated after it.

## Tests

```sh
just test     # offline by construction; no test calls the live API
```

145 tests, covering synth determinism by seed and the shape of both worlds; the
strategy and the solver on fixed fixtures with hand-computed expectations; each
primitive's schema validation rejecting bad output; the mock's rules stated
against the specification they implement; both engines' P&L on tiny fixtures;
that the pipeline passes prior outputs into later stages correctly; that no
ground truth reaches a judgment call; and the live client's translation in both
directions, against a stub System One server.

`just check` additionally runs `cargo fmt --check` and `clippy -D warnings`.

## `MockJev`

All rules in one file, `crates/jev-core/src/mock.rs`, with the thresholds
exposed as constants so a test can state the rule and the number in one place.
The action and the size come from a single rule evaluation — deriving them
separately lets them contradict each other. The most cautious applicable rule
wins.

## Cost

A full 90-day forex run is 858 typed calls — 214 decisions at four calls each,
plus two `Explain` calls for the report. Battery is 452: 90 days at five calls
each, plus the same two. Use `--limit N` to judge only the first N days when
running live.
