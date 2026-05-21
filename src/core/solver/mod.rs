mod candidates;
pub mod score;

use std::collections::HashSet;

use crate::core::feedback::compute_feedback;
use crate::core::filter::filter_by_history;
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

pub use score::{compare_final, compare_one_ply, score_one_ply, GuessScore};

use candidates::{select_guess_candidates, two_ply_candidate_indices, CandidateBuffer};
use score::score_two_ply;

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub word: Word,
    pub entropy: f64,
    pub expected_remaining: f64,
}

thread_local! {
    static CANDIDATE_SCRATCH: std::cell::RefCell<CandidateBuffer> =
        std::cell::RefCell::new(CandidateBuffer::new());
}

pub fn suggest_guess(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    hard_mode: bool,
) -> Option<Suggestion> {
    if remaining_answers.is_empty() {
        return None;
    }

    if history.is_empty() && remaining_answers.len() == word_lists.answers.len() {
        return Some(word_lists.opening_suggestion());
    }

    Some(compute_suggestion(
        word_lists,
        remaining_answers,
        history,
        hard_mode,
    ))
}

pub fn compute_suggestion(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    hard_mode: bool,
) -> Suggestion {
    if remaining_answers.len() == 1 {
        let word = remaining_answers[0];
        return Suggestion {
            word,
            entropy: 0.0,
            expected_remaining: 1.0,
        };
    }

    CANDIDATE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let guess_candidates = select_guess_candidates(
            word_lists,
            remaining_answers,
            history,
            hard_mode,
            &mut scratch,
        );

        let remaining_set: HashSet<Word> = remaining_answers.iter().copied().collect();

        let one_ply_scores: Vec<GuessScore> = guess_candidates
            .iter()
            .map(|&guess| score_one_ply(word_lists, guess, remaining_answers, &remaining_set))
            .collect();

        let refine_indices = two_ply_candidate_indices(&one_ply_scores, remaining_answers.len());

        let mut refined_scores = Vec::with_capacity(refine_indices.len());
        for idx in refine_indices {
            refined_scores.push(score_two_ply(
                word_lists,
                one_ply_scores[idx],
                remaining_answers,
                &remaining_set,
            ));
        }

        let best = refined_scores
            .into_iter()
            .max_by(|a, b| compare_final(*a, *b))
            .expect("at least one guess candidate");

        Suggestion {
            word: best.word,
            entropy: best.two_ply_entropy,
            expected_remaining: best.expected_remaining,
        }
    })
}

pub fn auto_solve(word_lists: &WordLists, target: Word) -> Option<Vec<(Word, Pattern)>> {
    let mut history = Vec::new();
    let max_turns = 6;

    for _ in 0..max_turns {
        let remaining = filter_by_history(&word_lists.answers, &history);

        let suggestion = suggest_guess(word_lists, &remaining, &history, false)?;
        let guess = suggestion.word;
        let pattern = compute_feedback(guess, target);
        history.push((guess, pattern));

        if pattern.is_win() {
            return Some(history);
        }
    }

    if history.last().map(|(_, p)| p.is_win()).unwrap_or(false) {
        Some(history)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::words::WordLists;

    fn w(s: &str) -> Word {
        Word::from_str(s).unwrap()
    }

    #[test]
    fn opening_guess_is_valid() {
        let lists = WordLists::load();
        assert!(lists.is_valid_guess(lists.opening_guess()));
    }

    #[test]
    fn suggests_from_remaining() {
        let lists = WordLists::load();
        let remaining = vec![w("crane"), w("grape")];
        let suggestion = suggest_guess(&lists, &remaining, &[], false).unwrap();
        assert!(remaining.contains(&suggestion.word) || lists.is_valid_guess(suggestion.word));
    }

    #[test]
    fn opening_is_instant_in_hard_mode() {
        let lists = WordLists::load();
        let suggestion = suggest_guess(&lists, &lists.answers, &[], true).unwrap();
        assert_eq!(suggestion.word, lists.opening_guess());
    }
}
