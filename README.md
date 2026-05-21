# NYTimes Wordle Solver

A Rust Wordle solver with a terminal UI (TUI). Uses official NYT Wordle word lists and an entropy-based guessing strategy.

## Requirements

- Rust 1.70+ (2021 edition)
- A terminal with UTF-8 support

## Build & Run

```bash
cargo run --release
```

Run tests:

```bash
cargo test
```

Full benchmark (all ~2,309 answers, ~1 minute in release):

```bash
cargo test --release --test integration -- --ignored --nocapture
```

## Modes

### Solver Aid

Enter the guesses you have played on [NYT Wordle](https://www.nytimes.com/games/wordle) along with the tile feedback. The solver filters the remaining possible answers and suggests an optimal next guess.

1. Type your 5-letter guess and press **Enter**
2. Set each tile to match NYT colors: **g** green, **y** yellow, **x** gray (or **Space** to cycle)
3. Press **Enter** to commit the turn

### Copilot

The solver picks each guess for you. Play the suggested word on NYT Wordle, then return and enter the feedback colors. Repeat until solved.

### Simulate

Fully autonomous solving with no NYT interaction:

- **Single word** — pick a target answer (or random) and watch the solver play
- **Benchmark** — run the solver against all NYT answers; shows average guesses, distribution, and hardest words

## Key Bindings

| Key | Action |
|-----|--------|
| `↑` / `↓` | Navigate menu or scroll candidates |
| `Enter` | Select / commit |
| `g` / `y` / `x` | Set tile green / yellow / gray |
| `Space` | Cycle tile color |
| `←` / `→` | Move feedback cursor |
| `h` | Toggle hard mode |
| `u` | Undo last turn |
| `r` | Reset game |
| `?` | Toggle help |
| `Esc` | Back to menu |
| `q` | Quit |

## Hard Mode

When enabled, each guess must use all known green letters in their positions and include all known yellow letters. Matches NYT hard mode rules.

## Word Lists

Bundled under `data/`:

- `answers.txt` — NYT solution words (~2,309)
- `allowed_guesses.txt` — additional valid guesses (~10,662)

Lists are extracted from NYT Wordle client data via community-maintained sources and may drift if NYT updates their dictionary.

## Algorithm

- **Feedback** — standard NYT rules including duplicate-letter handling
- **Filtering** — eliminate answers inconsistent with turn history
- **Guess selection** — maximize Shannon entropy over remaining answers; opening guess is **SLATE**

The solver solves 100% of NYT answer words within 6 guesses (verified by integration test).

## Project Structure

```
src/core/     — word lists, feedback, filtering, entropy solver, game state
src/tui/      — ratatui interface and screens
data/         — NYT word lists
tests/        — integration tests
```
