use std::collections::HashSet;

use crate::core::hard_mode::{filter_hard_mode_compliant, satisfies_hard_mode};
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

use super::score::{compare_one_ply, frequency_score, GuessScore};

const TOP_TWO_PLY: usize = 30;
const TOP_TWO_PLY_TIGHT: usize = 50;
const FULL_TWO_PLY_REMAINING: usize = 30;
const EARLY_GAME_REMAINING: usize = 500;
const EARLY_GAME_CANDIDATES: usize = 800;
/// When this many answers remain, only guess from the remaining set (hard-mode filtered).
/// When guesses left is tight, bias toward remaining answers (unless they share a suffix).
pub const TURNS_LEFT_REMAINING_SLACK: usize = 2;
const SHARED_SUFFIX_LEN: usize = 3;

pub struct CandidateBuffer {
    pub compliant_pool: Vec<Word>,
    pub small_remaining: Vec<Word>,
    pub early_game_pool: Vec<Word>,
    seen: HashSet<Word>,
}

impl CandidateBuffer {
    pub fn new() -> Self {
        Self {
            compliant_pool: Vec::new(),
            small_remaining: Vec::new(),
            early_game_pool: Vec::new(),
            seen: HashSet::new(),
        }
    }
}

fn tried_guesses(history: &[(Word, Pattern)]) -> HashSet<Word> {
    history.iter().map(|(g, _)| *g).collect()
}

fn exclude_prior_guesses(words: &mut Vec<Word>, tried: &HashSet<Word>) {
    words.retain(|w| !tried.contains(w));
}

pub(crate) fn shares_fixed_suffix(remaining: &[Word]) -> bool {
    if remaining.len() < 2 {
        return false;
    }
    let start = 5 - SHARED_SUFFIX_LEN;
    let suffix = &remaining[0].0[start..];
    remaining.iter().all(|w| &w.0[start..] == suffix)
}

fn should_use_remaining_only(remaining_len: usize, turns_left: usize) -> bool {
    remaining_len <= turns_left.saturating_add(TURNS_LEFT_REMAINING_SLACK)
}

fn union_unique(pool: &mut Vec<Word>, extra: &[Word], seen: &mut HashSet<Word>) {
    if extra.is_empty() {
        return;
    }
    seen.clear();
    seen.extend(pool.iter().copied());
    pool.reserve(extra.len());
    for &word in extra {
        if seen.insert(word) {
            pool.push(word);
        }
    }
}

fn fill_compliant_pool(
    scratch: &mut CandidateBuffer,
    word_lists: &WordLists,
    history: &[(Word, Pattern)],
) {
    if history.is_empty() {
        scratch.compliant_pool.clear();
        scratch
            .compliant_pool
            .extend_from_slice(&word_lists.guess_pool);
    } else {
        scratch.compliant_pool = filter_hard_mode_compliant(&word_lists.guess_pool, history);
    }
}

fn compliant_remaining_subset(
    remaining: &[Word],
    history: &[(Word, Pattern)],
    tried: &HashSet<Word>,
    out: &mut Vec<Word>,
) {
    out.clear();
    out.extend(
        remaining
            .iter()
            .copied()
            .filter(|&word| satisfies_hard_mode(word, history) && !tried.contains(&word)),
    );
    if out.is_empty() {
        out.extend(
            remaining
                .iter()
                .copied()
                .filter(|&word| satisfies_hard_mode(word, history)),
        );
        exclude_prior_guesses(out, tried);
    }
    out.sort();
}

/// Shared candidate-pool builder for main suggestions and 2-ply follow-ups.
fn build_guess_pool<'a>(
    word_lists: &'a WordLists,
    remaining: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    scratch: &'a mut CandidateBuffer,
) -> &'a [Word] {
    if remaining.is_empty() {
        return &[];
    }

    let tried = tried_guesses(history);

    if remaining.len() <= 2 {
        compliant_remaining_subset(remaining, history, &tried, &mut scratch.small_remaining);
        return &scratch.small_remaining;
    }

    if let Some(left) = turns_left {
        let suffix_cluster = shares_fixed_suffix(remaining);
        if should_use_remaining_only(remaining.len(), left)
            && !(suffix_cluster && remaining.len() > left)
        {
            compliant_remaining_subset(remaining, history, &tried, &mut scratch.small_remaining);
            return &scratch.small_remaining;
        }
    }

    fill_compliant_pool(scratch, word_lists, history);

    if turns_left.is_some_and(|left| shares_fixed_suffix(remaining) && remaining.len() > left) {
        exclude_prior_guesses(&mut scratch.compliant_pool, &tried);
        return &scratch.compliant_pool;
    }

    let pool = &scratch.compliant_pool;

    if remaining.len() > EARLY_GAME_REMAINING {
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
        union_unique(
            &mut scratch.early_game_pool,
            remaining,
            &mut scratch.seen,
        );
        exclude_prior_guesses(&mut scratch.early_game_pool, &tried);
        return &scratch.early_game_pool;
    }

    union_unique(&mut scratch.compliant_pool, remaining, &mut scratch.seen);
    exclude_prior_guesses(&mut scratch.compliant_pool, &tried);
    &scratch.compliant_pool
}

pub fn select_guess_candidates<'a>(
    word_lists: &'a WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    scratch: &'a mut CandidateBuffer,
) -> &'a [Word] {
    build_guess_pool(
        word_lists,
        remaining_answers,
        history,
        turns_left,
        scratch,
    )
}

pub fn followup_guess_pool<'a>(
    word_lists: &'a WordLists,
    subset: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    scratch: &'a mut CandidateBuffer,
) -> &'a [Word] {
    build_guess_pool(word_lists, subset, history, turns_left, scratch)
}

pub fn two_ply_candidate_indices(
    scores: &[GuessScore],
    remaining_len: usize,
    turns_left: Option<usize>,
) -> Vec<usize> {
    if remaining_len <= FULL_TWO_PLY_REMAINING {
        return (0..scores.len()).collect();
    }

    let top_n = if turns_left.is_some_and(|left| left <= 3) {
        TOP_TWO_PLY_TIGHT
    } else {
        TOP_TWO_PLY
    };

    let mut indices: Vec<usize> = (0..scores.len()).collect();
    indices.sort_by(|&a, &b| compare_one_ply(scores[b], scores[a]));
    indices.truncate(top_n.min(indices.len()));
    indices
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pattern::Pattern;
    use crate::core::words::WordLists;

    fn w(s: &str) -> Word {
        Word::parse(s).unwrap()
    }

    fn pat(s: &str) -> Pattern {
        Pattern::from_str(s).unwrap()
    }

    #[test]
    fn two_remaining_always_hard_mode_compliant() {
        let lists = WordLists::load();
        let history = vec![(w("slate"), pat("Gxxxx"))];
        let remaining = vec![w("snake"), w("stand")];
        let mut scratch = CandidateBuffer::new();
        let candidates = select_guess_candidates(&lists, &remaining, &history, None, &mut scratch);
        assert!(!candidates.is_empty());
        for &word in candidates {
            assert!(satisfies_hard_mode(word, &history));
        }
    }

    #[test]
    fn followup_pool_excludes_prior_guesses() {
        let lists = WordLists::load();
        let history = vec![(w("slate"), pat("Gxxxx")), (w("crane"), pat("xGYYx"))];
        let subset = vec![w("snake"), w("stand")];
        let mut scratch = CandidateBuffer::new();
        let pool = followup_guess_pool(&lists, &subset, &history, Some(2), &mut scratch);
        assert!(!pool.contains(&w("slate")));
        assert!(!pool.contains(&w("crane")));
        for &word in pool {
            assert!(satisfies_hard_mode(word, &history));
        }
    }

    #[test]
    fn small_remaining_candidates_are_hard_mode_compliant() {
        let lists = WordLists::load();
        let history = vec![(w("crane"), pat("xxxYx"))];
        let remaining = vec![w("snare"), w("snake")];
        let mut scratch = CandidateBuffer::new();
        let candidates =
            select_guess_candidates(&lists, &remaining, &history, Some(3), &mut scratch);
        assert!(!candidates.is_empty());
        for &word in candidates {
            assert!(satisfies_hard_mode(word, &history));
        }
    }

    #[test]
    fn followup_matches_main_pool_for_suffix_cluster() {
        let lists = WordLists::load();
        let remaining: Vec<Word> = ["bound", "found", "hound", "mound", "pound", "round"]
            .iter()
            .map(|s| w(s))
            .collect();
        let mut main_scratch = CandidateBuffer::new();
        let mut follow_scratch = CandidateBuffer::new();
        let main = select_guess_candidates(&lists, &remaining, &[], Some(2), &mut main_scratch);
        let follow = followup_guess_pool(&lists, &remaining, &[], Some(2), &mut follow_scratch);
        assert_eq!(main, follow);
    }
}
