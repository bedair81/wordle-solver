# NYTimes Wordle Solver

A Rust Wordle solver with a terminal UI (TUI) and headless CLI. Uses official NYT Wordle word lists and an entropy-based guessing strategy. **Hard mode is the default** (green positions fixed; yellow letters must appear in later guesses). Pass `--easy` to disable hard-mode constraints.

## Requirements

- Rust **1.88+** (edition 2021). `rust-toolchain.toml` pins **1.88.0** with `rustfmt` and `clippy` so rustup selects a compatible compiler.
- A TTY with UTF-8 support for the interactive TUI. Headless `suggest` does not need a TTY.

## Build & Run

Repo-root wrappers (pick one): `make start`, `npm start`, `./bin/wordle-solver`, or `cargo run --release`.

```bash
# Headless next-guess (works without a TTY; preferred for agents)
cargo run --release -- suggest --history slate:xxxxx
./bin/wordle-solver suggest --history slate:xxxxx
npm start -- suggest --history slate:xxxxx

# Interactive TUI (fails without a real TTY; use suggest instead)
cargo run --release
npm start
```

The `wordle-solver` binary is **not** on PATH until you install it:

```bash
make install          # cargo install --path . --locked
wordle-solver suggest --history slate:xxxxx
```

First process start builds a ~30MB pattern cache under `$WORDLE_SOLVER_CACHE` or `~/.cache/wordle-solver`. Later launches reuse it when the word lists match.

Debug / troubleshoot paths and word-list counts:

```bash
cargo run --release -- --healthcheck
```

## Validate

Default small-change loop (release; debug solver tests can take many minutes):

```bash
make check            # fmt + clippy -D warnings + cargo test --release
make test             # cargo test --release
make fmt              # cargo fmt --all -- --check
make lint             # cargo clippy --all-targets -- -D warnings
npm test
```

Scoped unit tests:

```bash
cargo test --release --lib word::
```

Verify interactive suggestion latency (each UI suggestion under 10s):

```bash
cargo run --release --bin suggestion-latency
```

Optional quality jobs (not the default loop). Strided sample (~50 answers):

```bash
cargo test --release auto_solves_strided_sample -- --ignored
```

Full-answer benchmark only (all 2351 answers, ~15–25 minutes in release):

```bash
cargo test --release --test integration auto_solves_all_answers_within_six_guesses -- --ignored --nocapture
```

Coverage report (`lcov.info`):

```bash
make coverage         # cargo llvm-cov --release --lcov
```

## Headless CLI

Patterns use `G` green, `Y` yellow, `X` gray (also `g`/`y`/`x`). History also accepts `/` and `=` separators.

```bash
# Next guess after turns (guess:pattern, comma-separated)
cargo run --release -- suggest --history slate:xxxxx,crimp:xxYxx

# Easy mode (no hard-mode letter constraints)
cargo run --release -- suggest --history slate:Gxxxx --easy

# Custom opening word when history is empty
cargo run --release -- suggest --opener crane

# TUI flags (need a TTY)
cargo run --release -- --easy --colorblind --opener slate --tui
```

Also: `--hard` (default), `--opening` (alias of `--opener`), `--healthcheck`.

## Screens

### Solver Aid

Enter the guesses you have played on [NYT Wordle](https://www.nytimes.com/games/wordle) along with the tile feedback. The solver filters the remaining possible answers and suggests an optimal next guess.

Suggestions after turn 1 run **off the UI thread** (status shows “computing…”) so quit/back/undo stay responsive.

1. Type letters for each **unlocked** tile (green tiles from prior turns are fixed in hard mode), then press **Enter**
2. Set each tile to match NYT colors: **g** green, **y** yellow, **x** gray (or **Space** to cycle)
3. Press **Enter** to commit the turn

### Copilot

The solver picks each guess for you. Play the suggested word on NYT Wordle, then return and enter the feedback colors.

## Key Bindings

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate menu or scroll candidates |
| `Enter` | Select / commit |
| `g` / `y` / `x` | Set tile green / yellow / gray (feedback phase) |
| `Space` | Cycle tile color (feedback phase) |
| `←` / `→` | Move feedback cursor (feedback phase) |
| `c` | Toggle colorblind tile mode (menu / feedback / view) |
| `u` | Undo last turn (feedback phase or game-over only) |
| `r` | Reset game (feedback phase or game-over only) |
| `?` | Toggle help |
| `Esc` | Back to menu |
| `q` | Quit |

While typing a guess, all letters (including `u`, `r`, and `c`) go into the word.

## Modes & session

- **Hard mode (default)** — greens fixed; prior yellow/green letters required
- **Easy mode** — `--easy` on CLI/TUI launch; no hard-mode constraints on guesses
- **Colorblind tiles** — high-contrast blue/orange palette plus `■` / `▲` / `·` marks (`c` to toggle)
- **Configurable opener** — `--opener WORD` (default `slate`)
- **Session restore** — in-progress games are saved under `~/.local/share/wordle-solver/session.txt` (override with the `WORDLE_SOLVER_SESSION` environment variable)

## Word Lists

Bundled under `data/`:

- `answers.txt` — NYT solution words (2351)
- `allowed_guesses.txt` — additional valid guesses (10662)

Refresh (also supports `--dry-run` and `--skip-tests`; a real update runs `cargo test`, not `--release`):

```bash
./scripts/update-wordlists.sh
./scripts/update-wordlists.sh --dry-run
./scripts/update-wordlists.sh --skip-tests
```

## Pattern cache

Guess×answer feedback is cached on disk under `~/.cache/wordle-solver/` (or the `WORDLE_SOLVER_CACHE` environment variable). First load builds and writes the cache; later launches load it when the word lists match.

## Algorithm

- **Pattern cache** — O(1) feedback lookups after load
- **Interactive budget** — UI suggestions target ≤ **10 seconds** (turn 2 after opener is typically instant via table)
- **Parallel scoring** — 1-ply and 2-ply candidate evaluation via Rayon
- **Smart candidate pool** — remaining-mass prepool + entropy ranking
- **2-ply lookahead** — refines top candidates under an adaptive interactive budget; ranks by expected remaining guesses when refined
- **Second-guess table** — precomputed best responses after the opener (`data/second_guess_table.rs`; regenerate with `cargo run --release --bin gen-second-guess`)
- **Selective 3-ply** — shallow extra ply on tight mid-game positions
- **Exact endgame** — minimax-style search when ≤8 answers remain
- **Opening guess** — configurable (default **SLATE**)

Run a full quality report:

```bash
cargo run --release --bin solver-quality
```

Other helper binaries: `opening-benchmark`, `pick-opener`, `opener-strided`.

## Project Structure

```
src/core/              — word lists, cache, feedback, filtering, entropy solver, game/session
src/cli.rs             — headless suggest / healthcheck
src/tui/               — ratatui interface (async suggestions, terminal guard)
src/bin/wordle-solver.rs — default binary
src/bin/               — quality, latency, opener, and table-generation tools
src/lib.rs             — library crate
data/                  — NYT word lists
tests/                 — integration and CLI tests
scripts/               — word-list refresh
.github/               — CI (fmt, clippy, release tests, cargo audit)
```

## Environment variables

| Environment variable | Purpose |
|----------------------|---------|
| `WORDLE_SOLVER_CACHE` | Pattern-cache directory |
| `WORDLE_SOLVER_SESSION` | Session file path |

Copy `.env.example` for the same names. Neither is required for a local run.
