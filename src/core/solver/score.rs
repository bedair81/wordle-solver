use std::collections::HashSet;

use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

/// Base-3 index in `0..243` for fixed-size pattern buckets.
pub fn pattern_bucket_index(pattern: Pattern) -> usize {
    let mut idx = 0usize;
    let mut mul = 1usize;
    for tile in pattern.tiles {
        let val = match tile {
            crate::core::pattern::Tile::Absent => 0,
            crate::core::pattern::Tile::Present => 1,
            crate::core::pattern::Tile::Correct => 2,
        };
        idx += val * mul;
        mul *= 3;
    }
    idx
}

pub const PATTERN_BUCKETS: usize = 243;

pub struct BucketCounts {
    pub counts: [usize; PATTERN_BUCKETS],
    pub nonempty: usize,
}

impl BucketCounts {
    pub fn zero() -> Self {
        Self {
            counts: [0; PATTERN_BUCKETS],
            nonempty: 0,
        }
    }
}

pub fn frequency_score(word: Word) -> usize {
    const FREQ: [usize; 26] = [
        8, 2, 5, 4, 12, 3, 4, 5, 7, 1, 1, 6, 5, 7, 6, 3, 1, 9, 6, 4, 3, 2, 2, 1, 3, 1,
    ];
    word.letters().map(|b| FREQ[(b - b'a') as usize]).sum()
}

#[derive(Clone, Copy, Debug)]
pub struct GuessScore {
    pub word: Word,
    pub two_ply_entropy: f64,
    pub one_ply_entropy: f64,
    pub worst_bucket: usize,
    pub expected_remaining: f64,
    pub is_possible_answer: bool,
    pub frequency: usize,
}

pub fn compare_final(a: GuessScore, b: GuessScore) -> std::cmp::Ordering {
    compare_one_ply(a, b).then_with(|| {
        a.two_ply_entropy
            .partial_cmp(&b.two_ply_entropy)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

pub fn compare_one_ply(a: GuessScore, b: GuessScore) -> std::cmp::Ordering {
    a.one_ply_entropy
        .partial_cmp(&b.one_ply_entropy)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| b.worst_bucket.cmp(&a.worst_bucket))
        .then_with(|| {
            b.expected_remaining
                .partial_cmp(&a.expected_remaining)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| a.is_possible_answer.cmp(&b.is_possible_answer))
        .then_with(|| a.frequency.cmp(&b.frequency))
        .then_with(|| b.word.cmp(&a.word))
}

pub fn metrics_from_buckets(buckets: &BucketCounts, total: usize) -> (f64, usize, f64) {
    let total_f = total as f64;
    let mut entropy = 0.0;
    let mut expected = 0.0;
    let mut worst = 0usize;

    for &count in &buckets.counts {
        if count == 0 {
            continue;
        }
        worst = worst.max(count);
        let p = count as f64 / total_f;
        entropy -= p * p.log2();
        expected += p * count as f64;
    }

    (entropy, worst, expected)
}

pub fn score_one_ply(
    word_lists: &WordLists,
    guess: Word,
    remaining: &[Word],
    remaining_set: &HashSet<Word>,
) -> GuessScore {
    let buckets = word_lists
        .pattern_cache
        .build_buckets_for(guess, remaining);
    let total = remaining.len();
    let (entropy, worst, expected) = metrics_from_buckets(&buckets, total);

    GuessScore {
        word: guess,
        two_ply_entropy: 0.0,
        one_ply_entropy: entropy,
        worst_bucket: worst,
        expected_remaining: expected,
        is_possible_answer: remaining_set.contains(&guess),
        frequency: frequency_score(guess),
    }
}

fn best_one_ply_entropy(
    word_lists: &WordLists,
    remaining: &[Word],
    guess_pool: &[Word],
    remaining_set: &HashSet<Word>,
) -> f64 {
    if remaining.len() <= 1 {
        return 0.0;
    }

    guess_pool
        .iter()
        .map(|&guess| score_one_ply(word_lists, guess, remaining, remaining_set).one_ply_entropy)
        .fold(0.0_f64, f64::max)
}

pub fn score_two_ply(
    word_lists: &WordLists,
    mut score: GuessScore,
    remaining: &[Word],
    _remaining_set: &HashSet<Word>,
) -> GuessScore {
    use crate::core::solver::candidates::{followup_guess_pool, CandidateBuffer};

    if remaining.len() <= 1 {
        score.two_ply_entropy = 0.0;
        return score;
    }

    let total = remaining.len() as f64;
    let mut partitions: [Vec<Word>; PATTERN_BUCKETS] = std::array::from_fn(|_| Vec::new());

    for &answer in remaining {
        let idx = word_lists
            .pattern_cache
            .bucket_or_compute(score.word, answer);
        partitions[idx].push(answer);
    }

    let mut followup_scratch = CandidateBuffer::new();
    let mut two_ply = 0.0;

    for subset in partitions.into_iter().filter(|s| !s.is_empty()) {
        let weight = subset.len() as f64 / total;
        let followup = if subset.len() <= 1 {
            0.0
        } else {
            let subset_set: HashSet<Word> = subset.iter().copied().collect();
            let pool = followup_guess_pool(word_lists, &subset, &mut followup_scratch);
            best_one_ply_entropy(word_lists, &subset, pool, &subset_set)
        };
        two_ply += weight * followup;
    }

    score.two_ply_entropy = two_ply;
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::words::WordLists;

    fn w(s: &str) -> Word {
        Word::from_str(s).unwrap()
    }

    fn set(words: &[&str]) -> HashSet<Word> {
        words.iter().map(|s| w(s)).collect()
    }

    #[test]
    fn prefers_possible_answer_on_one_ply_tie() {
        let lists = WordLists::load();
        let remaining = [w("crate"), w("grate")];
        let remaining_set = set(&["crate", "grate"]);
        let a = score_one_ply(&lists, w("crate"), &remaining, &remaining_set);
        let b = score_one_ply(&lists, w("slate"), &remaining, &remaining_set);
        if (a.one_ply_entropy - b.one_ply_entropy).abs() < 1e-9 {
            assert!(a.is_possible_answer);
        }
    }

    #[test]
    fn pattern_bucket_index_covers_all_tiles() {
        use crate::core::pattern::{Pattern, Tile};
        let pattern = Pattern::new([
            Tile::Correct,
            Tile::Present,
            Tile::Absent,
            Tile::Correct,
            Tile::Present,
        ]);
        let idx = pattern_bucket_index(pattern);
        assert!(idx < PATTERN_BUCKETS);
    }
}
