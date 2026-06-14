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

/// When ≤ this many turns remain, `compare_final` prefers minimax bucket sizing
/// over entropy (endgame positions where a single oversized bucket loses the game).
const TIGHT_TURNS_PARTITION_CUTOFF: usize = 4;

/// 1-ply entropies within this gap (bits) are treated as tied; consult 2-ply next.
/// Small enough to catch near-ties without overriding clear entropy winners.
const TWO_PLY_TIE_EPSILON: f64 = 0.015;

/// Largest bucket after a guess must be solvable in the turns still available after it.
pub(crate) fn partition_sufficient(max_bucket: usize, turns_left: usize) -> bool {
    max_bucket <= turns_left.saturating_sub(1).max(1)
}

pub fn compare_final(
    a: GuessScore,
    b: GuessScore,
    turns_left: Option<usize>,
) -> std::cmp::Ordering {
    // Endgame minimax: when turns are tight, prefer guesses that keep every feedback
    // bucket small enough to finish. Applies regardless of remaining count (unlike
    // `ENDGAME_PROBE_MAX_REMAINING` heuristics in mod.rs) because an oversized bucket
    // loses even mid-game if only a few guesses remain.
    if let Some(left) = turns_left {
        if left <= TIGHT_TURNS_PARTITION_CUTOFF && a.worst_bucket != b.worst_bucket {
            let a_ok = partition_sufficient(a.worst_bucket, left);
            let b_ok = partition_sufficient(b.worst_bucket, left);
            match (a_ok, b_ok) {
                (true, false) => return std::cmp::Ordering::Greater,
                (false, true) => return std::cmp::Ordering::Less,
                _ => {}
            }
            // Both sufficient or both insufficient: smaller worst bucket wins.
            return b.worst_bucket.cmp(&a.worst_bucket);
        }
    }

    // Near-tied 1-ply entropy: 2-ply breaks the tie before falling back to 1-ply metrics.
    // This can override a slightly better worst_bucket when entropies are within epsilon.
    let ent_gap = (a.one_ply_entropy - b.one_ply_entropy).abs();
    if ent_gap <= TWO_PLY_TIE_EPSILON {
        a.two_ply_entropy
            .partial_cmp(&b.two_ply_entropy)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| compare_one_ply(a, b))
    } else {
        compare_one_ply(a, b)
    }
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
        .then_with(|| {
            if a.is_possible_answer && b.is_possible_answer {
                a.word.cmp(&b.word)
            } else {
                std::cmp::Ordering::Equal
            }
        })
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
    let buckets = word_lists.pattern_cache.build_buckets_for(guess, remaining);
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

/// Best follow-up entropy for 2-ply scoring. Uses full `compare_one_ply` ordering to
/// pick the follow-up guess, but returns only the entropy scalar. Tie-breakers
/// (worst bucket, answer preference, etc.) affect the result only when follow-up
/// entropies are equal. Follow-up comparison stays 1-ply-centric (not `compare_final`
/// with turns-left) to avoid 3-ply cost inside 2-ply evaluation.
fn best_followup_one_ply(
    word_lists: &WordLists,
    remaining: &[Word],
    guess_pool: &[Word],
    remaining_set: &HashSet<Word>,
) -> f64 {
    if remaining.len() <= 1 {
        return 0.0;
    }

    debug_assert!(
        !guess_pool.is_empty(),
        "follow-up pool empty with {}-word subset",
        remaining.len()
    );

    guess_pool
        .iter()
        .map(|&guess| score_one_ply(word_lists, guess, remaining, remaining_set))
        .max_by(|a, b| compare_one_ply(*a, *b))
        .map(|s| s.one_ply_entropy)
        .unwrap_or(0.0)
}

pub fn score_two_ply(
    word_lists: &WordLists,
    mut score: GuessScore,
    remaining: &[Word],
    _remaining_set: &HashSet<Word>,
    history: &[(Word, crate::core::pattern::Pattern)],
    turns_left: Option<usize>,
) -> GuessScore {
    use crate::core::feedback::compute_feedback;
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
            let pattern = compute_feedback(score.word, subset[0]);
            let mut extended = history.to_vec();
            extended.push((score.word, pattern));
            let followup_turns = turns_left.map(|left| left.saturating_sub(1));
            let pool = followup_guess_pool(
                word_lists,
                &subset,
                &extended,
                followup_turns,
                &mut followup_scratch,
            );
            best_followup_one_ply(word_lists, &subset, pool, &subset_set)
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

    fn gs(
        word: &str,
        two_ply: f64,
        one_ply: f64,
        worst: usize,
        expected: f64,
        is_answer: bool,
    ) -> GuessScore {
        GuessScore {
            word: w(word),
            two_ply_entropy: two_ply,
            one_ply_entropy: one_ply,
            worst_bucket: worst,
            expected_remaining: expected,
            is_possible_answer: is_answer,
            frequency: 0,
        }
    }

    #[test]
    fn prefers_possible_answer_on_one_ply_tie() {
        let answer = gs("crate", 0.0, 1.0, 2, 1.0, true);
        let probe = gs("slate", 0.0, 1.0, 2, 1.0, false);
        assert_eq!(compare_one_ply(answer, probe), std::cmp::Ordering::Greater);
        assert_eq!(compare_one_ply(probe, answer), std::cmp::Ordering::Less);
    }

    #[test]
    fn compare_one_ply_prefers_smaller_worst_bucket_on_entropy_tie() {
        let better = gs("crate", 0.0, 1.0, 1, 1.5, false);
        let worse = gs("slate", 0.0, 1.0, 3, 1.5, false);
        assert_eq!(compare_one_ply(better, worse), std::cmp::Ordering::Greater);
    }

    #[test]
    fn partition_sufficient_boundary_cases() {
        assert!(partition_sufficient(1, 1));
        assert!(!partition_sufficient(2, 1));
        assert!(partition_sufficient(1, 2));
        assert!(partition_sufficient(2, 3));
        assert!(!partition_sufficient(4, 3));
    }

    #[test]
    fn compare_final_prefers_two_ply_on_entropy_tie() {
        let a = gs("slate", 2.5, 1.0, 4, 3.0, false);
        let b = gs("crane", 2.0, 1.01, 4, 3.0, true);
        assert_eq!(compare_final(a, b, None), std::cmp::Ordering::Greater);
        assert_eq!(compare_final(b, a, None), std::cmp::Ordering::Less);
    }

    #[test]
    fn compare_final_two_ply_tie_falls_back_to_one_ply() {
        let a = gs("slate", 2.0, 1.0, 4, 3.0, false);
        let b = gs("crane", 2.0, 1.01, 4, 3.0, true);
        assert_eq!(compare_final(a, b, None), std::cmp::Ordering::Less);
    }

    #[test]
    fn compare_final_epsilon_boundary() {
        let high_two_ply = gs("slate", 2.0, 1.0, 4, 3.0, false);
        let low_two_ply = gs("crane", 1.0, 1.014, 4, 3.0, true);
        assert_eq!(
            compare_final(high_two_ply, low_two_ply, None),
            std::cmp::Ordering::Greater,
            "gap 0.014 is within epsilon"
        );

        let outside = gs("crane", 1.0, 1.016, 4, 3.0, true);
        assert_eq!(
            compare_final(high_two_ply, outside, None),
            std::cmp::Ordering::Less,
            "gap 0.016 exceeds epsilon; higher 1-ply entropy wins"
        );
    }

    #[test]
    fn compare_final_prefers_sufficient_partition_when_turns_tight() {
        let good = gs("slate", 0.0, 1.5, 2, 2.0, false);
        let bad = gs("crane", 0.0, 2.0, 5, 2.0, true);
        assert_eq!(
            compare_final(good, bad, Some(3)),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compare_final_both_sufficient_prefers_smaller_worst_bucket() {
        let smaller = gs("slate", 0.0, 1.0, 2, 2.0, false);
        let larger = gs("crane", 0.0, 3.0, 3, 2.0, true);
        assert_eq!(
            compare_final(smaller, larger, Some(4)),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compare_final_both_insufficient_prefers_smaller_worst_bucket() {
        let smaller = gs("slate", 0.0, 1.0, 5, 2.0, false);
        let larger = gs("crane", 0.0, 3.0, 8, 2.0, true);
        assert_eq!(
            compare_final(smaller, larger, Some(3)),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compare_final_partition_branch_active_at_four_not_five_turns() {
        let good = gs("slate", 0.0, 1.0, 2, 2.0, false);
        let bad = gs("crane", 0.0, 3.0, 6, 2.0, true);
        assert_eq!(
            compare_final(good, bad, Some(4)),
            std::cmp::Ordering::Greater,
            "partition branch active at 4 turns"
        );
        assert_eq!(
            compare_final(good, bad, Some(5)),
            std::cmp::Ordering::Less,
            "at 5 turns higher entropy wins despite worse bucket"
        );
    }

    #[test]
    fn compare_final_partition_applies_with_large_remaining() {
        let good = gs("slate", 0.0, 0.5, 2, 50.0, false);
        let bad = gs("crane", 0.0, 3.0, 20, 50.0, true);
        assert_eq!(
            compare_final(good, bad, Some(4)),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compare_final_equal_worst_bucket_uses_two_ply_within_epsilon_at_tight_turns() {
        let high_two_ply = gs("slate", 2.5, 1.0, 3, 2.0, false);
        let low_two_ply = gs("crane", 1.0, 1.01, 3, 2.0, true);
        assert_eq!(
            compare_final(high_two_ply, low_two_ply, Some(3)),
            std::cmp::Ordering::Greater,
            "equal worst_bucket skips partition branch; epsilon + 2-ply decides"
        );
    }

    #[test]
    fn compare_final_epsilon_can_override_worst_bucket_outside_tight_turns() {
        let better_two_ply = gs("slate", 3.0, 1.0, 5, 3.0, false);
        let worse_two_ply = gs("crane", 1.0, 1.01, 2, 3.0, true);
        assert_eq!(
            compare_final(better_two_ply, worse_two_ply, None),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn score_two_ply_zero_for_single_remaining() {
        let lists = WordLists::load();
        let remaining = [w("crate")];
        let remaining_set = set(&["crate"]);
        let score = score_one_ply(&lists, w("slate"), &remaining, &remaining_set);
        let refined = score_two_ply(&lists, score, &remaining, &remaining_set, &[], None);
        assert_eq!(refined.two_ply_entropy, 0.0);
    }

    #[test]
    fn score_two_ply_positive_for_multi_remaining() {
        let lists = WordLists::load();
        let remaining = [w("crate"), w("grate"), w("trace")];
        let remaining_set = set(&["crate", "grate", "trace"]);
        let score = score_one_ply(&lists, w("slate"), &remaining, &remaining_set);
        let refined = score_two_ply(&lists, score, &remaining, &remaining_set, &[], None);
        assert!(refined.two_ply_entropy > 0.0);
        assert_eq!(refined.one_ply_entropy, score.one_ply_entropy);
    }

    fn score_refined_ound(
        lists: &WordLists,
        guess: Word,
        remaining: &[Word],
        remaining_set: &HashSet<Word>,
    ) -> GuessScore {
        let score = score_one_ply(lists, guess, remaining, remaining_set);
        score_two_ply(lists, score, remaining, remaining_set, &[], Some(3))
    }

    #[test]
    fn compare_final_orders_real_scored_guesses() {
        let lists = WordLists::load();
        let remaining = [
            w("bound"),
            w("found"),
            w("hound"),
            w("mound"),
            w("pound"),
            w("round"),
            w("sound"),
            w("wound"),
        ];
        let remaining_set: HashSet<Word> = remaining.iter().copied().collect();
        let slate = score_refined_ound(&lists, w("slate"), &remaining, &remaining_set);
        let taint = score_refined_ound(&lists, w("taint"), &remaining, &remaining_set);
        assert_eq!(
            compare_final(slate, taint, Some(3)),
            std::cmp::Ordering::Greater,
            "slate (worst=7) beats taint trap (worst=8)"
        );

        let guesses = [w("bound"), w("sound"), w("taint"), w("slate")];
        let refined: Vec<GuessScore> = guesses
            .iter()
            .map(|&word| score_refined_ound(&lists, word, &remaining, &remaining_set))
            .collect();
        let best = refined
            .iter()
            .max_by(|a, b| compare_final(**a, **b, Some(3)))
            .expect("at least one guess");
        assert_eq!(best.word, w("slate"));
        assert_eq!(best.worst_bucket, 7);
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
