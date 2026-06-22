//! Measure per-suggestion latency for interactive UI budgeting.
use std::time::{Duration, Instant};

use wordle_solver::core::feedback::compute_feedback;
use wordle_solver::core::filter::filter_by_history;
use wordle_solver::core::game::GameState;
use wordle_solver::core::pattern::Pattern;
use wordle_solver::core::solver::{suggest_guess_interactive, INTERACTIVE_SUGGESTION_BUDGET};
use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

fn w(s: &str) -> Word {
    Word::parse(s).unwrap()
}

fn pat(s: &str) -> Pattern {
    Pattern::from_str(s).unwrap()
}

fn timed_interactive(
    lists: &WordLists,
    remaining: &[Word],
    history: &[(Word, Pattern)],
    turns_left: usize,
) -> Duration {
    let start = Instant::now();
    let _ = suggest_guess_interactive(lists, remaining, history, turns_left);
    start.elapsed()
}

fn main() {
    let full_scan = std::env::args().any(|a| a == "--full");
    let lists = WordLists::load();
    let mut samples: Vec<(String, Duration)> = Vec::new();

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
        let lists_arc = std::sync::Arc::new(lists.clone());
        let mut game = GameState::new(lists_arc);
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
                max_label = format!("{} ({} rem)", target, remaining.len());
            }
        }
        samples.push((format!("FULL UI turn-2 max: {max_label}"), max_d));
    }

    samples.sort_by_key(|b| std::cmp::Reverse(b.1));

    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    println!("Interactive suggestion latency ({profile} build):");
    println!("Budget: {:.0}s\n", INTERACTIVE_SUGGESTION_BUDGET.as_secs_f64());

    for (label, d) in &samples {
        let ms = d.as_secs_f64() * 1000.0;
        let flag = if *d > INTERACTIVE_SUGGESTION_BUDGET {
            " *** OVER BUDGET ***"
        } else if *d > Duration::from_secs(3) {
            " (slow)"
        } else {
            ""
        };
        println!("  {ms:8.1} ms  {label}{flag}");
    }

    let max = samples[0].1;
    if max > INTERACTIVE_SUGGESTION_BUDGET {
        eprintln!(
            "\nFAIL: max latency {:.2}s exceeds {:.0}s budget",
            max.as_secs_f64(),
            INTERACTIVE_SUGGESTION_BUDGET.as_secs_f64()
        );
        std::process::exit(1);
    }
    println!(
        "\nOK: max latency {:.2}s within {:.0}s budget",
        max.as_secs_f64(),
        INTERACTIVE_SUGGESTION_BUDGET.as_secs_f64()
    );
}
