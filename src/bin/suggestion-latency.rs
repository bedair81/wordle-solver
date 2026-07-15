//! Measure per-suggestion latency for interactive UI budgeting.
use std::time::{Duration, Instant};

use wordle_solver::core::feedback::compute_feedback;
use wordle_solver::core::filter::filter_by_history;
use wordle_solver::core::game::GameState;
use wordle_solver::core::pattern::Pattern;
use wordle_solver::core::solver::{
    interactive_suggestion_budget, suggest_guess_interactive, INTERACTIVE_SUGGESTION_BUDGET,
};
use wordle_solver::core::word::Word;
use wordle_solver::core::words::{shared_word_lists, OPENING_GUESS};

fn w(s: &str) -> Word {
    Word::parse(s).unwrap()
}

fn pat(s: &str) -> Pattern {
    Pattern::from_str(s).unwrap()
}

fn timed_interactive(
    lists: &wordle_solver::core::words::WordLists,
    remaining: &[Word],
    history: &[(Word, Pattern)],
    turns_left: usize,
) -> Duration {
    let start = Instant::now();
    let _ = suggest_guess_interactive(lists, remaining, history, turns_left, false, OPENING_GUESS);
    start.elapsed()
}

fn main() {
    let full_scan = std::env::args().any(|a| a == "--full");
    let lists = shared_word_lists();
    let mut samples: Vec<(String, Duration)> = Vec::new();
    let budget = interactive_suggestion_budget();

    // Turn 2: typical post-opening (worst-case pool size) — UI path
    {
        let history = vec![(w("slate"), pat("xxxxx"))];
        let remaining = filter_by_history(&lists.answers, &history);
        let d = timed_interactive(&lists, &remaining, &history, 5);
        samples.push((
            format!("UI turn 2 / slate all gray / {} remaining", remaining.len()),
            d,
        ));
    }

    // Endgame *ound cluster
    {
        let remaining: Vec<Word> = [
            "bound", "found", "hound", "mound", "pound", "round", "sound", "wound",
        ]
        .iter()
        .map(|s| w(s))
        .collect();
        let d = timed_interactive(&lists, &remaining, &[], 3);
        samples.push(("UI endgame *ound / 8 remaining / 3 left".into(), d));
    }

    // GameState path (exact TUI code path)
    {
        let mut game = GameState::new(std::sync::Arc::clone(&lists));
        game.record_turn(w("slate"), pat("xxxxx")).unwrap();
        let start = Instant::now();
        let _ = game.suggest_next();
        samples.push(("GameState.suggest_next turn 2".into(), start.elapsed()));
    }

    if full_scan {
        let mut max_d = Duration::ZERO;
        let mut max_label = String::new();
        for &target in &lists.answers {
            let pattern = compute_feedback(w("slate"), target);
            let history = vec![(w("slate"), pattern)];
            let remaining = filter_by_history(&lists.answers, &history);
            if remaining.is_empty() {
                continue;
            }
            let d = timed_interactive(&lists, &remaining, &history, 5);
            if d > max_d {
                max_d = d;
                max_label = format!("{target} remaining={}", remaining.len());
            }
        }
        samples.push((format!("FULL max after slate ({max_label})"), max_d));
    }

    println!("Interactive suggestion latency (budget {budget:?})");
    println!("Legacy const INTERACTIVE_SUGGESTION_BUDGET = {INTERACTIVE_SUGGESTION_BUDGET:?}");
    let mut ok = true;
    for (label, d) in &samples {
        let status = if *d <= budget { "OK" } else { "OVER" };
        if *d > budget {
            ok = false;
        }
        println!("  [{status}] {label}: {:.3}s", d.as_secs_f64());
    }
    if !ok {
        std::process::exit(1);
    }
}
