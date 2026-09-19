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

# Build the UI, then serve it and the API from one process on :8787.
desk: ui-build serve

# The HTTP API, offline. Serves ui/dist when it has been built.
serve *ARGS:
    cargo run --release -p server -- {{ARGS}}

# The same, defaulting to the live System One API. Needs TYPESAFE_API_KEY.
serve-live *ARGS:
    cargo run --release -p server -- --live {{ARGS}}

# The Vite dev server on :5173, proxying /api to a running `just serve`.
ui-dev:
    cd ui && npm install && npm run dev

# Build the UI into ui/dist, which `just serve` picks up.
ui-build:
    cd ui && npm install && npm run build

# Typecheck the UI without building it.
ui-check:
    cd ui && npm install && npm run check

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

# Format, lint and test the workspace.
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

# The same, plus the UI's typecheck. Needs node.
check-all: check ui-check

fmt:
    cargo fmt --all

# Remove generated reports and audit logs.
clean-out:
    rm -rf out

# Remove the UI's build output and its dependencies.
clean-ui:
    rm -rf ui/dist ui/node_modules
