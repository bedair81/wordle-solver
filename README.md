# NYTimes Wordle Solver

A Rust Wordle solver with a terminal UI (TUI) and headless CLI. Uses official NYT Wordle word lists and an entropy-based guessing strategy. **Hard mode is the default** (green positions fixed; yellow letters must appear in later guesses). Pass `--easy` to disable hard-mode constraints.

## Requirements

- Rust 1.74+ (2021 edition)
- A terminal with UTF-8 support (for the TUI)

## Build & Run

```bash
cargo run --release                 # interactive TUI
cargo run --release -- suggest --history slate:xxxxx
```

Run tests:

```bash
cargo test --release
```

Verify interactive suggestion latency (each UI suggestion under 10s):

```bash
cargo run --release --bin suggestion-latency
```

Fast CI runs hard-case smoke tests only. For strided quality sampling:

```bash
cargo test --release auto_solves_strided_sample -- --ignored
```

Full benchmark (all ~2,309 answers, ~15–25 minutes in release):

```bash
cargo test --release --test integration -- --ignored --nocapture
```

## Headless CLI

```bash
# Next guess after turns (guess:G/Y/X pattern, comma-separated)
wordle-solver suggest --history slate:xxxxx,crane:xxYxx

# Easy mode (no hard-mode letter constraints)
wordle-solver suggest --history slate:Gxxxx --easy

# Custom opening word when history is empty
wordle-solver suggest --opener crane

# TUI flags
wordle-solver --easy --colorblind --opener slate
```

Patterns use `G` green, `Y` yellow, `X` gray (also `g`/`y`/`x`).

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
- **Session restore** — in-progress games are saved under `~/.local/share/wordle-solver/session.txt` (override with `WORDLE_SOLVER_SESSION`)

## Word Lists

Bundled under `data/`:

- `answers.txt` — NYT solution words (~2,350)
- `allowed_guesses.txt` — additional valid guesses (~10,662)

Refresh:

```bash
./scripts/update-wordlists.sh
```

## Pattern cache

Guess×answer feedback is cached on disk under `~/.cache/wordle-solver/` (or `$WORDLE_SOLVER_CACHE`). First load builds and writes the cache; later launches load it when the word lists match.

## Algorithm

- **Pattern cache** — O(1) feedback lookups after load
- **Interactive budget** — UI suggestions target ≤ **10 seconds** (typical release ~0.5–2s after turn 2)
- **Parallel scoring** — 1-ply and 2-ply candidate evaluation via Rayon
- **Smart candidate pool** — early-game heuristic prepool + entropy ranking
- **2-ply lookahead** — refines top candidates within the interactive budget
- **Exact endgame** — minimax-style search when ≤8 answers remain
- **Opening guess** — configurable (default **SLATE**)

Run a full quality report:

```bash
cargo run --release --bin solver-quality
```

## Project Structure

```
src/core/     — word lists, cache, feedback, filtering, entropy solver, game/session
src/tui/      — ratatui interface (async suggestions, terminal guard)
src/cli.rs    — headless suggest command
data/         — NYT word lists
tests/        — integration tests
.github/      — CI (fmt, clippy, release tests)
```

## Environment

| Variable | Purpose |
|----------|---------|
| `WORDLE_SOLVER_CACHE` | Pattern-cache directory |
| `WORDLE_SOLVER_SESSION` | Session file path |
