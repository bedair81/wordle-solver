# NYTimes Wordle Solver

A Rust Wordle solver with a terminal UI (TUI). Uses official NYT Wordle word lists and an entropy-based guessing strategy. All play follows NYT hard mode rules (green positions fixed; yellow letters must appear in later guesses).

NYT also lets you disable hard mode in the official game; this project always assumes hard mode is on, matching the default NYT setting.

## Requirements

- Rust 1.70+ (2021 edition)
- A terminal with UTF-8 support

## Build & Run

```bash
cargo run --release
```

Run tests:

```bash
cargo test --release
```

Verify interactive suggestion latency (each UI suggestion under 10s):

```bash
cargo run --release --bin suggestion-latency
```

Fast CI runs hard-case smoke tests only (~5–6s release, ~1–2 min debug). For strided quality sampling:

```bash
cargo test --release auto_solves_strided_sample -- --ignored
```

Full benchmark (all ~2,309 answers, ~15–25 minutes in release):

```bash
cargo test --release --test integration -- --ignored --nocapture
```

## Screens

### Solver Aid

Enter the guesses you have played on [NYT Wordle](https://www.nytimes.com/games/wordle) along with the tile feedback. The solver filters the remaining possible answers and suggests an optimal next guess.

The first suggested guess appears **after you commit turn 1** (opening play is yours on NYT). From turn 2 onward, suggestions follow each committed turn.

1. Type letters for each **unlocked** tile (green tiles from prior turns are fixed), then press **Enter**
2. Set each tile to match NYT colors: **g** green, **y** yellow, **x** gray (or **Space** to cycle)
3. Press **Enter** to commit the turn

Your guess must satisfy NYT hard mode before feedback entry (greens in place, all prior yellow letters included). Guesses may be words outside our cached guess list (with a warning); only answers are restricted to the NYT answer list.

### Copilot

The solver picks each guess for you. Play the suggested word on NYT Wordle, then return and enter the feedback colors. Repeat until solved.

Copilot only suggests words from our bundled guess list (NYT-legal guesses). Solver Aid allows typing any five letters, which is useful when NYT accepts a word we do not have cached.

## Key Bindings

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate menu or scroll candidates |
| `Enter` | Select / commit |
| `g` / `y` / `x` | Set tile green / yellow / gray (feedback phase) |
| `Space` | Cycle tile color (feedback phase) |
| `←` / `→` | Move feedback cursor (feedback phase) |
| `u` | Undo last turn (feedback phase or game-over only) |
| `r` | Reset game (feedback phase or game-over only) |
| `?` | Toggle help |
| `Esc` | Back to menu |
| `q` | Quit |

While typing a guess, all letters (including `u` and `r`) go into the word — undo during typing is intentionally disabled so those keys can be used in words like `crumb` or `urged`. `u` undo and `r` reset are only active in the feedback phase or after the game ends.

## Guess Rules (NYT Hard Mode)

- **Green** letters must stay in the same position in every later guess
- **Yellow** letters must appear in every later guess (including duplicates when multiple yellows were shown)

The solver only suggests guesses that satisfy these constraints. Solver Aid rejects guesses that violate them.

## Word Lists

Bundled under `data/`:

- `answers.txt` — NYT solution words (~2,309)
- `allowed_guesses.txt` — additional valid guesses (~10,662)

Lists are extracted from NYT Wordle client data via community-maintained sources and may drift if NYT updates their dictionary.

## Algorithm

Quality-first solver, optimized for interactive use:

- **Pattern cache** — all guess×answer feedback precomputed at startup (~0.5s once in release), then O(1) lookups per scoring step
- **Interactive budget** — each UI suggestion completes within **10 seconds** (`GameState::suggest_next`); typical release latency ~0.8s after turn 2
- **Smart candidate pool** — early game uses top 1000 heuristic guesses (580 in debug builds for the UI path) from the hard-mode-compliant pool plus all remaining answers; late game uses the compliant pool or remaining answers when few candidates remain
- **2-ply lookahead** — top 45 candidates (60 when ≤3 turns left; all guesses when ≤25 answers remain); 1-ply metrics primary, 2-ply tie-breaker with hard-mode-aware follow-ups
- **Endgame heuristics** — minimax bucket sizing, off-list partition probes for suffix clusters, and forced-win search on tiny sets when turns are tight
- **Opening guess** — **SLATE** (instant, no startup computation)
- **Multi-criteria scoring** — entropy, minimax worst bucket, expected remaining, win-aware tie-break

Run a full quality report:

```bash
cargo run --release --bin solver-quality
```

## Notes

- **Feedback** — standard NYT rules including duplicate-letter handling
- **Filtering** — eliminate answers inconsistent with turn history

The solver solves 100% of NYT answer words within 6 guesses with NYT hard-mode constraints always enforced. Verify with:

```bash
cargo test --release --test integration -- --ignored --nocapture
cargo run --release --bin solver-quality
```

## Project Structure

```
src/core/     — word lists, feedback, filtering, entropy solver, game state
src/tui/      — ratatui interface and screens
data/         — NYT word lists
tests/        — integration tests
```