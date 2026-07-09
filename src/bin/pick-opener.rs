//! Fast 1-ply opener ranking (entropy / expected remaining / expected guesses).
//! Full auto-solve comparison remains in `opening-benchmark`.

use std::collections::HashSet;

use wordle_solver::core::solver::score_one_ply;
use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

fn main() {
    let lists = WordLists::load();
    let remaining = lists.answers.as_slice();
    let remaining_set: HashSet<Word> = remaining.iter().copied().collect();

    let openers = [
        "slate", "crane", "trace", "stare", "raise", "crate", "salet", "dealt", "tares", "arise",
        "soare", "roate", "orate", "reast", "slate",
    ];

    let mut ranked = Vec::new();
    for name in openers {
        let Some(w) = Word::parse(name) else { continue };
        if !lists.is_valid_guess(w) {
            eprintln!("skip invalid/not-in-pool: {name}");
            continue;
        }
        let s = score_one_ply(&lists, w, remaining, &remaining_set);
        ranked.push((w, s));
    }

    ranked.sort_by(|a, b| {
        b.1.one_ply_entropy
            .partial_cmp(&a.1.one_ply_entropy)
            .unwrap()
            .then_with(|| {
                a.1.expected_remaining
                    .partial_cmp(&b.1.expected_remaining)
                    .unwrap()
            })
            .then_with(|| {
                a.1.expected_guesses
                    .partial_cmp(&b.1.expected_guesses)
                    .unwrap()
            })
            .then_with(|| a.0.cmp(&b.0))
    });

    println!(
        "{:<8} {:>10} {:>12} {:>12} {:>8}",
        "opener", "entropy", "exp_remain", "exp_guesses", "worst"
    );
    for (w, s) in &ranked {
        println!(
            "{:<8} {:>10.4} {:>12.2} {:>12.4} {:>8}",
            w.as_str(),
            s.one_ply_entropy,
            s.expected_remaining,
            s.expected_guesses,
            s.worst_bucket
        );
    }
    if let Some((best, s)) = ranked.first() {
        println!();
        println!(
            "BEST_OPENER={} entropy={:.4} exp_remain={:.2}",
            best.as_str(),
            s.one_ply_entropy,
            s.expected_remaining
        );
    }
}
