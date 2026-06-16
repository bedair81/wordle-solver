//! Offline benchmark for opening guesses. Run once when tuning the solver.
use wordle_solver::core::feedback::compute_feedback;
use wordle_solver::core::filter::filter_by_history;
use wordle_solver::core::hard_mode::satisfies_hard_mode;
use wordle_solver::core::pattern::Pattern;
use wordle_solver::core::solver::suggest_guess_with_turns;
use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

fn auto_solve_with_opener(
    word_lists: &WordLists,
    target: Word,
    opener: Word,
) -> Option<Vec<(Word, Pattern)>> {
    let mut history = Vec::new();
    let max_turns = 6;

    for turn in 0..max_turns {
        let remaining = filter_by_history(&word_lists.answers, &history);
        let turns_left = max_turns - history.len();

        if turns_left == 1 {
            for &word in &remaining {
                if satisfies_hard_mode(word, &history) {
                    let pattern = compute_feedback(word, target);
                    history.push((word, pattern));
                    if pattern.is_win() {
                        return Some(history);
                    }
                    history.pop();
                }
            }
            return None;
        }

        let guess = if turn == 0 {
            opener
        } else {
            suggest_guess_with_turns(word_lists, &remaining, &history, Some(turns_left))?.word
        };

        let pattern = compute_feedback(guess, target);
        history.push((guess, pattern));
        if pattern.is_win() {
            return Some(history);
        }
    }

    None
}

fn main() {
    let lists = WordLists::load();
    let openers = [
        "slate", "crane", "trace", "stare", "raise", "crate", "salet", "dealt", "tares", "arise",
    ];

    let mut results: Vec<(Word, f64, usize, usize)> = Vec::new();

    for name in openers {
        let opener = match Word::parse(name) {
            Some(w) if lists.is_valid_guess(w) => w,
            _ => continue,
        };

        let mut total = 0usize;
        let mut failures = 0usize;
        let mut worst = 0usize;

        for &target in &lists.answers {
            match auto_solve_with_opener(&lists, target, opener) {
                Some(h) => {
                    let n = h.len();
                    total += n;
                    worst = worst.max(n);
                }
                None => failures += 1,
            }
        }

        if failures > 0 {
            eprintln!("{name}: FAILED {failures} words");
            continue;
        }

        let avg = total as f64 / lists.answers.len() as f64;
        results.push((opener, avg, total, worst));
    }

    results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    println!("Opening guess benchmark ({} answers):\n", lists.answers.len());
    for (word, avg, total, worst) in &results {
        println!(
            "  {word}: avg={avg:.4} total={total} worst={worst}"
        );
    }

    if let Some((best, avg, _, _)) = results.first() {
        println!("\nBest opener: {best} (avg={avg:.4})");
    }
}
