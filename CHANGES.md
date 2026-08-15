# Changes report — wordle-solver improvement pass

## Changes and expected impact

| Change | Expected impact |
|--------|-----------------|
| **Non-blocking suggestions** (`spawn_suggestion_job`, `PlayState::request_suggestion` / `poll_suggestion`, generation invalidation) | UI stays responsive during scoring; quit/back/undo work while “computing…”; stale results after undo/reset discarded |
| **Panic-safe terminal** (`TerminalGuard` + panic hook) | Raw mode / alt screen restored on panic or Drop so the shell is usable |
| **On-disk pattern cache** (`core/cache.rs`, `WordLists::load_with_config`) | Faster cold/warm starts; second load is a cache hit when word lists match |
| **Process-wide shared lists** (`shared_word_lists`) | Tests and repeated callers reuse one pattern matrix (much faster test suite) |
| **Rayon parallel 1-ply / 2-ply scoring** | Lower suggestion latency on multi-core machines (observed ~0.5s turn-2 release path) |
| **Headless CLI** `suggest --history …` | Scriptable solver without TUI; hard-mode compliance checked on hard path |
| **Easy mode** (`--easy`, `GameState::easy_mode`) | Optional play without hard-mode constraints; hard remains default |
| **Configurable opener** (`--opener`, `AppConfig.opening`) | Opening guess no longer fixed only to SLATE when configured |
| **Session save/restore** (`core/session.rs`, TUI paths) | Resume in-progress games across restarts |
| **Colorblind tile mode** (`c` toggle, high-contrast palette + symbols) | More accessible tile feedback presentation |
| **Exact endgame search** (≤8 remaining) | Force-win minimax with real INF + offlist probes; *ound cluster max_bucket≤4 (not remaining-answer trap) |
| **Centralized knobs** (`AppConfig`, `SolverConfig`) | Tuning/docs live in one place instead of scattered constants |
| **Play state split** (`play_state.rs` vs `aid.rs` render) | Pure play logic unit-testable without a terminal |
| **Game history single source** (`turns` only; `history()` derived) | Less drift risk between dual vectors |
| **CI** (`.github/workflows/ci.yml`) | fmt + clippy (-D warnings) + `cargo test --release` on PRs |
| **Cargo.toml metadata** | license, repo, keywords, rust-version 1.88, version 0.2.0 |
| **Expanded tests** | Cache hit/miss, session round-trip, async job lifecycle, easy mode, opener, feedback properties, colorblind |

## Verification evidence (scratch)

- `cargo-test-release.log` — full release suite green (hard-case smoke included)
- `suggest-cli.log` — CLI prints 5-letter suggestions; opener/easy paths work
- `feature-tests.log` — cache, session, job, easy mode, colorblind, async play-state
- `lint.log` — fmt check + clippy -D warnings
- `latency.log` — interactive budget respected (~0.5s turn 2)
- `tui-launch.log` — build + structural markers for async/guard

## Residual suggestions (minimal)

1. Optional scheduled CI job for ignored full-answer quality benchmark (15–25 min).
2. Criterion/divan benches if you want historical latency charts beyond `suggestion-latency`.
3. Mouse support for tile coloring (out of scope for this pass).

## Agent compatibility (this pass)

- MSRV / `rust-toolchain.toml` pin **1.88.0** (locked crates need rustc 1.88+).
- Repo-root `Makefile` / `package.json` / `./bin/wordle-solver` startup and validate targets.
- Headless-first docs; TUI exits with a TTY hint; `--healthcheck` for debug.
- Extra `tests/` files, clippy/rustfmt config, pre-commit hooks, `cargo audit` in CI.
