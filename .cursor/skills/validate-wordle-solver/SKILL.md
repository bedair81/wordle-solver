---
name: validate-wordle-solver
description: Run the repo-root validation loop for the Rust Wordle solver.
---

# Validate wordle-solver

From the repository root:

1. `make fmt` — `cargo fmt --all -- --check`
2. `make lint` — `cargo clippy --all-targets -- -D warnings`
3. `make test` — `cargo test --release`

Or one shot: `make check`.

For a single module: `cargo test --release --lib word::`.

Use `cargo run --release -- --healthcheck` to confirm cache and word-list paths.
