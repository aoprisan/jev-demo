# jev-desk — the solver owns the numbers, Jev owns the judgment, the schema owns the contract.
#
# Every recipe has a `-mock` variant that runs against the offline rule-based
# backend. The plain variants call the live System One API and need
# TYPESAFE_API_KEY; `--mock` is the only switch between them.

set dotenv-load := true

_default:
    @just --list --unsorted

# Both domains, then one Explain call summarising the day for Compliance.
demo-all *ARGS:
    cargo run --release -p cli -- all {{ARGS}}

# Both domains, offline.
demo-all-mock *ARGS:
    cargo run --release -p cli -- all --mock {{ARGS}}

# Forex: full simulation, then one CPI day replayed with and without the event.
demo-fx *ARGS:
    cargo run --release -p cli -- fx {{ARGS}}

# Forex, offline.
demo-fx-mock *ARGS:
    cargo run --release -p cli -- fx --mock {{ARGS}}

# Battery: full simulation, then one day replayed with a grid notice added.
demo-battery *ARGS:
    cargo run --release -p cli -- battery {{ARGS}}

# Battery, offline.
demo-battery-mock *ARGS:
    cargo run --release -p cli -- battery --mock {{ARGS}}

# The whole test suite. Offline by construction: no test calls the live API.
test *ARGS:
    cargo test --workspace {{ARGS}}

# Same as `test`; the suite never needs a key.
test-mock *ARGS:
    cargo test --workspace {{ARGS}}

# Print the standing instructions for each primitive.
prompts:
    cargo run --release -p cli -- prompts

# Print the JSON Schema of each primitive output.
schemas:
    cargo run --release -p cli -- schemas

# Format, lint and test.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

fmt:
    cargo fmt --all

# Remove generated reports and audit logs.
clean-out:
    rm -rf out
