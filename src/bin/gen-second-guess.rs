//! Generate `data/second_guess_table.rs` for the configured opening word.
//!
//! Usage:
//!   cargo run --release --bin gen-second-guess
//!   cargo run --release --bin gen-second-guess -- --opener salet
//!
//! Writes a Rust array expression of `Option<Word>` length 243.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::PathBuf;

use wordle_solver::core::feedback::compute_feedback;
use wordle_solver::core::filter::filter_by_history;
use wordle_solver::core::solver::score::{pattern_bucket_index, PATTERN_BUCKETS};
use wordle_solver::core::solver::compute_suggestion_live;
use wordle_solver::core::word::Word;
use wordle_solver::core::words::{WordLists, OPENING_GUESS};

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut opener = OPENING_GUESS;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--opener" {
            i += 1;
            opener = Word::parse(&args[i]).expect("invalid --opener");
        }
        i += 1;
    }

    assert_eq!(PATTERN_BUCKETS, 243);
    let lists = WordLists::load();
    assert!(
        lists.is_valid_guess(opener),
        "opener {opener} not in guess pool"
    );

    // Group answers by first-feedback pattern after the opener.
    let mut by_pattern: HashMap<usize, Vec<Word>> = HashMap::new();
    for &answer in &lists.answers {
        let pattern = compute_feedback(opener, answer);
        if pattern.is_win() {
            continue;
        }
        let idx = pattern_bucket_index(pattern);
        by_pattern.entry(idx).or_default().push(answer);
    }

    let mut table: Vec<Option<Word>> = vec![None; PATTERN_BUCKETS];
    let mut filled = 0usize;

    let mut patterns: Vec<_> = by_pattern.into_iter().collect();
    patterns.sort_by_key(|(idx, _)| *idx);

    eprintln!(
        "Generating second-guess table for opener={opener} ({} patterns)...",
        patterns.len()
    );

    for (idx, answers) in patterns {
        // Reconstruct a representative history via any answer in the bucket.
        let sample = answers[0];
        let pattern = compute_feedback(opener, sample);
        debug_assert_eq!(pattern_bucket_index(pattern), idx);
        let history = vec![(opener, pattern)];
        let remaining = filter_by_history(&lists.answers, &history);
        debug_assert_eq!(remaining.len(), answers.len());

        let turns_left = Some(5usize);
        // Live search only — must not consult the table being regenerated.
        let suggestion = compute_suggestion_live(&lists, &remaining, &history, turns_left, false);
        match suggestion {
            Some(s) => {
                table[idx] = Some(s.word);
                filled += 1;
                eprintln!(
                    "  pattern {idx:3} remaining={:4} -> {}",
                    remaining.len(),
                    s.word
                );
            }
            None => {
                eprintln!("  pattern {idx:3} remaining={:4} -> NONE", remaining.len());
            }
        }
    }

    let mut out = String::from("[\n");
    for (i, slot) in table.iter().enumerate() {
        match slot {
            Some(w) => out.push_str(&format!("    Some(Word(*b\"{}\"))", w.as_str())),
            None => out.push_str("    None"),
        }
        if i + 1 != table.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push(']');

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/second_guess_table.rs");
    fs::write(&path, out).expect("write second_guess_table.rs");
    eprintln!(
        "Wrote {} ({} / {} slots filled) for opener={opener}",
        path.display(),
        filled,
        PATTERN_BUCKETS
    );
}
