use std::collections::HashSet;

use crate::core::hard_mode::satisfies_hard_mode;
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

use super::candidates::shares_fixed_suffix;
use super::score::score_one_ply;
use super::Suggestion;

pub(crate) struct SolverContext<'a> {
    pub(crate) word_lists: &'a WordLists,
    pub(crate) remaining: &'a [Word],
    pub(crate) remaining_set: HashSet<Word>,
    pub(crate) history: &'a [(Word, Pattern)],
    pub(crate) turns_left: Option<usize>,
    pub(crate) easy_mode: bool,
    pub(crate) suffix_cluster: bool,
    pub(crate) tried: HashSet<Word>,
}

impl<'a> SolverContext<'a> {
    pub(crate) fn new(
        word_lists: &'a WordLists,
        remaining: &'a [Word],
        history: &'a [(Word, Pattern)],
        turns_left: Option<usize>,
        easy_mode: bool,
    ) -> Self {
        Self {
            word_lists,
            remaining,
            remaining_set: remaining.iter().copied().collect(),
            history,
            turns_left,
            easy_mode,
            suffix_cluster: shares_fixed_suffix(remaining),
            tried: history.iter().map(|(g, _)| *g).collect(),
        }
    }

    pub(crate) fn suggestion_from_score(&self, word: Word) -> Suggestion {
        let score = score_one_ply(self.word_lists, word, self.remaining, &self.remaining_set);
        Suggestion {
            word,
            entropy: score.one_ply_entropy,
            expected_remaining: score.expected_remaining,
        }
    }

    pub(crate) fn hard_mode_ok(&self, word: Word) -> bool {
        self.easy_mode || satisfies_hard_mode(word, self.history)
    }
}
