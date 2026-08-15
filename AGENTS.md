# Agent guidelines

## Toolchain

Rust 1.88+ (`rust-version` in Cargo.toml). `rust-toolchain.toml` pins 1.88.0 with rustfmt and clippy so rustup switches automatically. If crates fail a rustc-version gate, run `rustup update` and retry in this directory.

## Start

Headless (no TTY): `cargo run --release -- suggest --history slate:xxxxx`, `./bin/wordle-solver suggest --history slate:xxxxx`, or `npm start -- suggest --history slate:xxxxx`.

TUI needs a real terminal: `cargo run --release` or `npm start`. Without a TTY the binary exits with that headless hint.

First run writes a ~30MB pattern cache under `$WORDLE_SOLVER_CACHE` or `~/.cache/wordle-solver`. Later runs reuse it. Debug with `cargo run --release -- --healthcheck`.

`wordle-solver` is not on PATH unless you `make install` (`cargo install --path .`).

## Validate a small change

Do not use debug `cargo test` for solver tests (can take many minutes). Use release.

- `make check` — fmt, clippy `-D warnings`, `cargo test --release`
- `make test` or `npm test` — tests only
- scoped: `cargo test --release --lib word::`
- `make fmt` / `make lint` separately

CI also runs `cargo audit`. Before push: `cargo fmt --all -- --check` must pass.

Ignored tests (`auto_solves_strided_sample`, full-answer benchmarks) are quality jobs, not the default loop.

## Environment variables

See `.env.example`. `WORDLE_SOLVER_CACHE` and `WORDLE_SOLVER_SESSION` override cache and session paths. CI sets `WORDLE_SOLVER_CACHE` to an isolated temp dir.

## Layout

`src/core` solver, `src/cli.rs` + `src/tui` UI, `src/bin/wordle-solver.rs` entry, `tests/` integration.
