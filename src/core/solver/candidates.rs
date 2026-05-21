use crate::core::hard_mode::filter_hard_mode_compliant;
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

use super::score::{frequency_score, GuessScore};

const TOP_TWO_PLY: usize = 30;
const FULL_TWO_PLY_REMAINING: usize = 30;
const EARLY_GAME_REMAINING: usize = 500;
const EARLY_GAME_CANDIDATES: usize = 800;

pub struct CandidateBuffer {
    pub hard_mode_pool: Vec<Word>,
    pub small_remaining: Vec<Word>,
    pub early_game_pool: Vec<Word>,
}

impl CandidateBuffer {
    pub fn new() -> Self {
        Self {
            hard_mode_pool: Vec::new(),
            small_remaining: Vec::new(),
            early_game_pool: Vec::new(),
        }
    }
}

pub fn select_guess_candidates<'a>(
    word_lists: &'a WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    hard_mode: bool,
    scratch: &'a mut CandidateBuffer,
) -> &'a [Word] {
    if remaining_answers.len() <= 2 {
        scratch.small_remaining.clear();
        scratch.small_remaining.extend_from_slice(remaining_answers);
        return &scratch.small_remaining;
    }

    let pool = if hard_mode {
        scratch.hard_mode_pool = filter_hard_mode_compliant(&word_lists.guess_pool, history);
        &scratch.hard_mode_pool
    } else {
        &word_lists.guess_pool
    };

    if remaining_answers.len() > EARLY_GAME_REMAINING {
        scratch.early_game_pool.clear();
        let mut scored: Vec<(Word, usize)> = pool
            .iter()
            .copied()
            .map(|w| (w, w.unique_letter_count() * 10 + frequency_score(w)))
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        scratch.early_game_pool.extend(
            scored
                .into_iter()
                .take(EARLY_GAME_CANDIDATES)
                .map(|(w, _)| w),
        );
        return &scratch.early_game_pool;
    }

    pool
}

pub fn two_ply_candidate_indices(scores: &[GuessScore], remaining_len: usize) -> Vec<usize> {
    use super::score::compare_one_ply;

    if remaining_len <= FULL_TWO_PLY_REMAINING {
        return (0..scores.len()).collect();
    }

    let mut indices: Vec<usize> = (0..scores.len()).collect();
    indices.sort_by(|&a, &b| compare_one_ply(scores[b], scores[a]));
    indices.truncate(TOP_TWO_PLY.min(indices.len()));
    indices
}

pub fn followup_guess_pool<'a>(
    word_lists: &'a WordLists,
    subset: &[Word],
    scratch: &'a mut CandidateBuffer,
) -> &'a [Word] {
    if subset.len() <= 50 {
        scratch.small_remaining.clear();
        scratch.small_remaining.extend_from_slice(subset);
        return &scratch.small_remaining;
    }

    if subset.len() > EARLY_GAME_REMAINING {
        scratch.early_game_pool.clear();
        let mut scored: Vec<(Word, usize)> = word_lists
            .guess_pool
            .iter()
            .copied()
            .map(|w| (w, w.unique_letter_count() * 10 + frequency_score(w)))
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        scratch.early_game_pool.extend(
            scored
                .into_iter()
                .take(EARLY_GAME_CANDIDATES)
                .map(|(w, _)| w),
        );
        return &scratch.early_game_pool;
    }

    &word_lists.guess_pool
}
