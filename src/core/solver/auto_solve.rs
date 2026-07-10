use crate::core::feedback::compute_feedback;
use crate::core::filter::filter_by_history;
use crate::core::hard_mode::satisfies_hard_mode;
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::{WordLists, OPENING_GUESS};

use super::suggest_guess_with_options;

pub fn auto_solve(word_lists: &WordLists, target: Word) -> Option<Vec<(Word, Pattern)>> {
    auto_solve_with_options(word_lists, target, false, OPENING_GUESS)
}

pub fn auto_solve_with_options(
    word_lists: &WordLists,
    target: Word,
    easy_mode: bool,
    opening: Word,
) -> Option<Vec<(Word, Pattern)>> {
    let mut history = Vec::new();
    let max_turns = 6;

    for _ in 0..max_turns {
        let remaining = filter_by_history(&word_lists.answers, &history);
        let turns_left = max_turns - history.len();

        if turns_left == 1 {
            for &word in &remaining {
                if easy_mode || satisfies_hard_mode(word, &history) {
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

        let suggestion = suggest_guess_with_options(
            word_lists,
            &remaining,
            &history,
            Some(turns_left),
            false,
            easy_mode,
            opening,
        )?;
        let guess = suggestion.word;
        let pattern = compute_feedback(guess, target);
        history.push((guess, pattern));

        if pattern.is_win() {
            return Some(history);
        }
    }

    None
}
