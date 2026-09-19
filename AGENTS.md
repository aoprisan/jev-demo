# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust 2021 workspace (Rust 1.88+) with a React/TypeScript UI.
- `crates/jev-core/`: typed judgment primitives, live/mock clients, schemas, audit records, and the standing guidance in `prompts/` (structured JSON, merged into each question's instructions).
- `crates/synth/`: deterministic synthetic data generation.
- `crates/fx/` and `crates/battery/`: domain features, strategies, solvers, and pipelines.
- `crates/report/`, `crates/cli/`, and `crates/server/`: report rendering, CLI commands, and the Axum API.
- `crates/*/tests/`: Rust integration tests.
- `ui/src/`: components, hooks, API types/client, and styles; `ui/dist/` is generated.
- `out/`: generated reports and audit logs, ignored by Git.

## Build, Test, and Development Commands

- `cargo build --workspace`: build all Rust crates.
- `just demo-all-mock`: run both domains offline and generate reports.
- `just desk`: build the UI and serve it with the API on port 8787.
- `just serve` and, separately, `just ui-dev`: run the offline API and Vite development server on port 5173.
- `just test`: run all Rust tests offline; use `cargo test -p fx` to focus on one crate.
- `just fmt`: format Rust code.
- `just check-all`: check Rust formatting, run Clippy with warnings denied, run tests, and typecheck the UI.
- `just ui-build`: typecheck and build the frontend.

## Coding Style & Naming Conventions

Use four-space Rust indentation and rustfmt's configured 100-column width. Use `snake_case` for modules/functions and `PascalCase` for types. Follow existing TypeScript style: two-space indentation, double quotes, semicolons, PascalCase components, and `useX` hooks. TypeScript uses strict checking; no frontend lint or formatter script is configured.

## Testing Guidelines

Use Rust `#[test]` and `#[tokio::test]`, with descriptive snake_case names. Keep tests offline through mocks or local stub servers. Preserve deterministic seeds, schema validation, and the prohibition on latent truth leaking into judgments. Update server API contract tests and `ui/src/api/types.ts` together when DTOs change. No numeric coverage threshold or frontend test runner is configured.

## Commit & Pull Request Guidelines

History uses concise, scope-prefixed subjects such as `server: ...` and `ui: ...`. Follow that pattern. PRs should describe behavior changes, list validation commands/results, link relevant issues, and include screenshots for UI changes.

## Architecture & Configuration

Keep numerical calculations in deterministic domain code and judgments in typed primitives. Never silently accept invalid model outputs. Live runs require `TYPESAFE_API_KEY`; keep credentials out of commits and use mock commands for routine development.
