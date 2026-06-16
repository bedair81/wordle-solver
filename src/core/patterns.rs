use std::collections::HashMap;

use crate::core::feedback::compute_feedback;
use crate::core::solver::score::pattern_bucket_index;
use crate::core::word::Word;

/// Precomputed guess×answer feedback bucket indices for fast entropy scoring.
#[derive(Clone)]
pub struct PatternCache {
    pub num_answers: usize,
    answer_index: HashMap<Word, usize>,
    guess_index: HashMap<Word, usize>,
    data: Vec<u8>,
}

impl PatternCache {
    pub fn build(answers: &[Word], guess_pool: &[Word]) -> Self {
        let num_answers = answers.len();
        let mut answer_index = HashMap::with_capacity(num_answers);
        for (i, &word) in answers.iter().enumerate() {
            answer_index.insert(word, i);
        }

        let mut guess_index = HashMap::with_capacity(guess_pool.len());
        for (i, &word) in guess_pool.iter().enumerate() {
            guess_index.insert(word, i);
        }

        let mut data = vec![0u8; guess_pool.len() * num_answers];
        for (gi, &guess) in guess_pool.iter().enumerate() {
            let row = gi * num_answers;
            for (ai, &answer) in answers.iter().enumerate() {
                let bucket = pattern_bucket_index(compute_feedback(guess, answer));
                debug_assert!(bucket < 243);
                data[row + ai] = bucket as u8;
            }
        }

        Self {
            num_answers,
            answer_index,
            guess_index,
            data,
        }
    }

    pub fn bucket(&self, guess: Word, answer: Word) -> Option<usize> {
        let gi = self.guess_index.get(&guess)?;
        let ai = self.answer_index.get(&answer)?;
        Some(self.data[gi * self.num_answers + ai] as usize)
    }

    pub fn bucket_or_compute(&self, guess: Word, answer: Word) -> usize {
        self.bucket(guess, answer)
            .unwrap_or_else(|| pattern_bucket_index(compute_feedback(guess, answer)))
    }

    pub fn build_buckets(
        &self,
        guess: Word,
        remaining: &[Word],
    ) -> Option<crate::core::solver::score::BucketCounts> {
        use crate::core::solver::score::{BucketCounts, PATTERN_BUCKETS};

        let gi = self.guess_index.get(&guess)?;
        let row = gi * self.num_answers;
        let mut buckets = BucketCounts::zero();

        for &answer in remaining {
            let ai = self.answer_index.get(&answer)?;
            let idx = self.data[row + ai] as usize;
            debug_assert!(idx < PATTERN_BUCKETS);
            if buckets.counts[idx] == 0 {
                buckets.nonempty += 1;
            }
            buckets.counts[idx] += 1;
        }

        Some(buckets)
    }

    /// Cached when possible; falls back to live feedback for guesses outside the pool.
    pub fn build_buckets_for(
        &self,
        guess: Word,
        remaining: &[Word],
    ) -> crate::core::solver::score::BucketCounts {
        use crate::core::solver::score::{BucketCounts, PATTERN_BUCKETS};

        if let Some(buckets) = self.build_buckets(guess, remaining) {
            return buckets;
        }

        let mut buckets = BucketCounts::zero();
        for &answer in remaining {
            let idx = self.bucket_or_compute(guess, answer);
            debug_assert!(idx < PATTERN_BUCKETS);
            if buckets.counts[idx] == 0 {
                buckets.nonempty += 1;
            }
            buckets.counts[idx] += 1;
        }
        buckets
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::words::WordLists;

    #[test]
    fn cache_matches_live_feedback() {
        use crate::core::feedback::compute_feedback;

        let lists = WordLists::load();
        let guess = Word::parse("slate").unwrap();
        let answer = Word::parse("crate").unwrap();
        let expected = pattern_bucket_index(compute_feedback(guess, answer));
        assert_eq!(lists.pattern_cache.bucket(guess, answer), Some(expected));
    }

    #[test]
    fn build_buckets_for_unknown_guess_matches_live() {
        let lists = WordLists::load();
        let guess = Word::parse("qqqqq").unwrap();
        let remaining = [
            Word::parse("agree").unwrap(),
            Word::parse("abbey").unwrap(),
        ];
        let live = lists.pattern_cache.build_buckets_for(guess, &remaining);
        assert!(live.nonempty >= 1);
    }
}
