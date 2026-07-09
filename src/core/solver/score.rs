use std::cell::RefCell;
use std::collections::HashSet;

use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

use crate::core::config::solver_config;

use super::candidates::{followup_guess_pool, CandidateBuffer};

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

/// Sentinel: multi-ply fields not computed yet.
pub const UNREFINED_EXPECTED_GUESSES: f64 = f64::INFINITY;

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

/// Letter + position mass over the current remaining answers (for early prepool ranking).
#[derive(Clone, Debug)]
pub struct RemainingMass {
    pub letter: [usize; 26],
    pub position: [[usize; 26]; 5],
}

impl RemainingMass {
    pub fn from_remaining(remaining: &[Word]) -> Self {
        let mut letter = [0usize; 26];
        let mut position = [[0usize; 26]; 5];
        for w in remaining {
            let mut seen = [false; 26];
            for (i, &b) in w.0.iter().enumerate() {
                let idx = (b - b'a') as usize;
                position[i][idx] += 1;
                if !seen[idx] {
                    seen[idx] = true;
                    letter[idx] += 1;
                }
            }
        }
        Self { letter, position }
    }

    /// Higher is better: unique-letter coverage of remaining mass + positional hits.
    pub fn score_word(&self, word: Word) -> usize {
        let mut used = [false; 26];
        let mut score = 0usize;
        for (i, &b) in word.0.iter().enumerate() {
            let idx = (b - b'a') as usize;
            score += self.position[i][idx].saturating_mul(4);
            if !used[idx] {
                used[idx] = true;
                score += self.letter[idx].saturating_mul(12);
            }
        }
        score + word.unique_letter_count().saturating_mul(8)
    }
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
    /// Expected total guesses from this position if we play `word` (including this guess).
    /// [`UNREFINED_EXPECTED_GUESSES`] when multi-ply has not been computed.
    pub expected_guesses: f64,
    /// True after multi-ply refinement filled `expected_guesses` / `two_ply_entropy`.
    pub refined: bool,
}

impl GuessScore {
    pub fn unrefined(
        word: Word,
        one_ply_entropy: f64,
        worst_bucket: usize,
        expected_remaining: f64,
        is_possible_answer: bool,
        frequency: usize,
    ) -> Self {
        Self {
            word,
            two_ply_entropy: 0.0,
            one_ply_entropy,
            worst_bucket,
            expected_remaining,
            is_possible_answer,
            frequency,
            expected_guesses: UNREFINED_EXPECTED_GUESSES,
            refined: false,
        }
    }
}

/// Prefer guessing from remaining answers only when the candidate pool is small.
const ANSWER_PREFERENCE_MAX_REMAINING: usize = 8;

/// Largest bucket after a guess must be solvable in the turns still available after it.
pub(crate) fn partition_sufficient(max_bucket: usize, turns_left: usize) -> bool {
    max_bucket <= turns_left.saturating_sub(1).max(1)
}

pub fn compare_final(
    a: GuessScore,
    b: GuessScore,
    turns_left: Option<usize>,
    remaining_len: usize,
) -> std::cmp::Ordering {
    // Endgame minimax: when turns are tight, prefer guesses that keep every feedback
    // bucket small enough to finish.
    if let Some(left) = turns_left {
        if left <= solver_config().tight_turns_partition_cutoff && a.worst_bucket != b.worst_bucket
        {
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

    // When both refined: prefer lower expected remaining guesses (average-case objective),
    // then higher follow-up entropy, then 1-ply metrics. This intentionally trusts multi-ply
    // over moderate 1-ply gaps (legacy epsilon was 0.022 and discarded most 2-ply work).
    if a.refined && b.refined {
        return b
            .expected_guesses
            .partial_cmp(&a.expected_guesses)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                a.two_ply_entropy
                    .partial_cmp(&b.two_ply_entropy)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                let a_total = a.one_ply_entropy + a.two_ply_entropy;
                let b_total = b.one_ply_entropy + b.two_ply_entropy;
                a_total
                    .partial_cmp(&b_total)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| compare_one_ply(a, b, remaining_len));
    }

    // Near-tied 1-ply when only one side refined (or neither): classic 2-ply entropy break.
    const MULTI_PLY_TIE_EPSILON: f64 = 0.08;
    let ent_gap = (a.one_ply_entropy - b.one_ply_entropy).abs();
    if ent_gap <= MULTI_PLY_TIE_EPSILON {
        let ord = a
            .two_ply_entropy
            .partial_cmp(&b.two_ply_entropy)
            .unwrap_or(std::cmp::Ordering::Equal);
        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }

    compare_one_ply(a, b, remaining_len)
}

pub fn compare_one_ply(a: GuessScore, b: GuessScore, remaining_len: usize) -> std::cmp::Ordering {
    a.one_ply_entropy
        .partial_cmp(&b.one_ply_entropy)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| b.worst_bucket.cmp(&a.worst_bucket))
        .then_with(|| {
            b.expected_remaining
                .partial_cmp(&a.expected_remaining)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| {
            if remaining_len <= ANSWER_PREFERENCE_MAX_REMAINING {
                a.is_possible_answer.cmp(&b.is_possible_answer)
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .then_with(|| {
            if remaining_len <= ANSWER_PREFERENCE_MAX_REMAINING
                && a.is_possible_answer
                && b.is_possible_answer
            {
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

/// 1-ply expected remaining guesses (including the current guess) from bucket counts.
/// Win bucket contributes 0 further guesses; size-1 leaves need 1 more; larger leaves use a
/// soft log-based estimate biased toward average-case (not pure minimax).
pub fn expected_guesses_from_buckets(
    buckets: &BucketCounts,
    total: usize,
    guess_is_possible_answer: bool,
) -> f64 {
    if total == 0 {
        return 0.0;
    }
    if total == 1 {
        return 1.0;
    }
    let total_f = total as f64;
    let win_idx = PATTERN_BUCKETS - 1; // all Correct = 242
    debug_assert_eq!(
        pattern_bucket_index(Pattern::new([
            crate::core::pattern::Tile::Correct;
            5
        ])),
        win_idx
    );

    let mut further = 0.0;
    for (idx, &count) in buckets.counts.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let p = count as f64 / total_f;
        if guess_is_possible_answer && idx == win_idx {
            continue;
        }
        if count == 1 {
            further += p * 1.0;
        } else {
            // Soft average-case depth: one more guess + fractional log2 (not full minimax).
            further += p * (1.0 + (count as f64).log2() * 0.45);
        }
    }
    1.0 + further
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
    let is_answer = remaining_set.contains(&guess);
    // Soft 1-ply expected-guesses estimate (not multi-ply refined).
    let eg = expected_guesses_from_buckets(&buckets, total, is_answer);

    GuessScore {
        word: guess,
        two_ply_entropy: 0.0,
        one_ply_entropy: entropy,
        worst_bucket: worst,
        expected_remaining: expected,
        is_possible_answer: is_answer,
        frequency: frequency_score(guess),
        expected_guesses: eg,
        refined: false,
    }
}

struct TwoPlyScratch {
    followup_buffer: CandidateBuffer,
    partitions: [Vec<Word>; PATTERN_BUCKETS],
    subset_set: HashSet<Word>,
    extended_history: Vec<(Word, Pattern)>,
}

impl TwoPlyScratch {
    fn new() -> Self {
        Self {
            followup_buffer: CandidateBuffer::new(),
            partitions: std::array::from_fn(|_| Vec::new()),
            subset_set: HashSet::new(),
            extended_history: Vec::new(),
        }
    }

    fn clear_partitions(&mut self) {
        for bucket in &mut self.partitions {
            bucket.clear();
        }
    }
}

thread_local! {
    static TWO_PLY_SCRATCH: RefCell<TwoPlyScratch> = RefCell::new(TwoPlyScratch::new());
}

fn compare_followup(
    a: GuessScore,
    b: GuessScore,
    turns_left: Option<usize>,
    remaining_len: usize,
) -> std::cmp::Ordering {
    if let Some(left) = turns_left {
        if left <= solver_config().tight_turns_partition_cutoff && a.worst_bucket != b.worst_bucket
        {
            let a_ok = partition_sufficient(a.worst_bucket, left);
            let b_ok = partition_sufficient(b.worst_bucket, left);
            match (a_ok, b_ok) {
                (true, false) => return std::cmp::Ordering::Greater,
                (false, true) => return std::cmp::Ordering::Less,
                _ => return b.worst_bucket.cmp(&a.worst_bucket),
            }
        }
    }
    // Follow-ups: entropy-first (average-case), then softer expected-guesses as tie-break.
    compare_one_ply(a, b, remaining_len).then_with(|| {
        b.expected_guesses
            .partial_cmp(&a.expected_guesses)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

struct FollowupPick {
    entropy: f64,
    expected_guesses: f64,
}

/// Best follow-up for 2-ply scoring: entropy + expected-guesses estimate.
fn best_followup_one_ply(
    word_lists: &WordLists,
    remaining: &[Word],
    guess_pool: &[Word],
    remaining_set: &HashSet<Word>,
    turns_left: Option<usize>,
) -> FollowupPick {
    if remaining.len() <= 1 {
        return FollowupPick {
            entropy: 0.0,
            expected_guesses: remaining.len() as f64,
        };
    }

    debug_assert!(
        !guess_pool.is_empty(),
        "follow-up pool empty with {}-word subset",
        remaining.len()
    );

    guess_pool
        .iter()
        .map(|&guess| score_one_ply(word_lists, guess, remaining, remaining_set))
        .max_by(|a, b| compare_followup(*a, *b, turns_left, remaining.len()))
        .map(|s| FollowupPick {
            entropy: s.one_ply_entropy,
            expected_guesses: s.expected_guesses,
        })
        .unwrap_or(FollowupPick {
            entropy: 0.0,
            expected_guesses: 1.0 + (remaining.len() as f64).log2(),
        })
}

fn score_two_ply_with_scratch(
    scratch: &mut TwoPlyScratch,
    word_lists: &WordLists,
    mut score: GuessScore,
    remaining: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    easy_mode: bool,
) -> GuessScore {
    use crate::core::feedback::compute_feedback;

    if remaining.len() <= 1 {
        score.two_ply_entropy = 0.0;
        score.expected_guesses = remaining.len() as f64;
        score.refined = true;
        return score;
    }

    scratch.clear_partitions();

    for &answer in remaining {
        let idx = word_lists
            .pattern_cache
            .bucket_or_compute(score.word, answer);
        scratch.partitions[idx].push(answer);
    }

    let total = remaining.len() as f64;
    let mut accumulated_entropy = 0.0;
    let mut accumulated_further = 0.0;
    let followup_turns = turns_left.map(|left| left.saturating_sub(1));
    let win_idx = PATTERN_BUCKETS - 1;

    for (idx, subset) in scratch.partitions.iter().enumerate() {
        if subset.is_empty() {
            continue;
        }
        let weight = subset.len() as f64 / total;

        // Correct guess: no further turns.
        if subset.len() == 1 && subset[0] == score.word {
            debug_assert_eq!(idx, win_idx);
            continue;
        }
        if subset.len() == 1 {
            accumulated_further += weight * 1.0;
            continue;
        }

        scratch.subset_set.clear();
        scratch.subset_set.extend(subset.iter().copied());
        let pattern = compute_feedback(score.word, subset[0]);
        scratch.extended_history.clear();
        scratch.extended_history.extend_from_slice(history);
        scratch.extended_history.push((score.word, pattern));
        let pool = followup_guess_pool(
            word_lists,
            subset,
            &scratch.extended_history,
            followup_turns,
            easy_mode,
            &mut scratch.followup_buffer,
        );
        let followup = best_followup_one_ply(
            word_lists,
            subset,
            pool,
            &scratch.subset_set,
            followup_turns,
        );
        accumulated_entropy += weight * followup.entropy;
        accumulated_further += weight * followup.expected_guesses;
    }

    score.two_ply_entropy = accumulated_entropy;
    score.expected_guesses = 1.0 + accumulated_further;
    score.refined = true;
    score
}

pub fn score_two_ply(
    word_lists: &WordLists,
    score: GuessScore,
    remaining: &[Word],
    _remaining_set: &HashSet<Word>,
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
) -> GuessScore {
    score_two_ply_with_mode(
        word_lists,
        score,
        remaining,
        _remaining_set,
        history,
        turns_left,
        false,
    )
}

pub fn score_two_ply_with_mode(
    word_lists: &WordLists,
    score: GuessScore,
    remaining: &[Word],
    _remaining_set: &HashSet<Word>,
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    easy_mode: bool,
) -> GuessScore {
    TWO_PLY_SCRATCH.with(|scratch| {
        score_two_ply_with_scratch(
            &mut scratch.borrow_mut(),
            word_lists,
            score,
            remaining,
            history,
            turns_left,
            easy_mode,
        )
    })
}

/// Shallow 3-ply: re-score top candidates using 2-ply expected-guesses inside each first partition.
///
/// Builds owned partitions first so nested [`score_two_ply_with_mode`] can use thread-local
/// scratch without re-entrant `RefCell` borrows.
pub fn score_three_ply_with_mode(
    word_lists: &WordLists,
    score: GuessScore,
    remaining: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    easy_mode: bool,
) -> GuessScore {
    use crate::core::feedback::compute_feedback;

    if remaining.len() <= 2 {
        return score_two_ply_with_mode(
            word_lists,
            score,
            remaining,
            &HashSet::new(),
            history,
            turns_left,
            easy_mode,
        );
    }

    let mut partitions: Vec<Vec<Word>> = vec![Vec::new(); PATTERN_BUCKETS];
    for &answer in remaining {
        let idx = word_lists
            .pattern_cache
            .bucket_or_compute(score.word, answer);
        partitions[idx].push(answer);
    }

    let total = remaining.len() as f64;
    let mut accumulated_further = 0.0;
    let mut accumulated_entropy = 0.0;
    let followup_turns = turns_left.map(|left| left.saturating_sub(1));
    let follow_cap = solver_config().three_ply_followup_cap;

    for subset in &partitions {
        if subset.is_empty() {
            continue;
        }
        let weight = subset.len() as f64 / total;
        if subset.len() == 1 && subset[0] == score.word {
            continue;
        }
        if subset.len() == 1 {
            accumulated_further += weight * 1.0;
            continue;
        }

        let pattern = compute_feedback(score.word, subset[0]);
        let mut extended_history = history.to_vec();
        extended_history.push((score.word, pattern));
        let subset_set: HashSet<Word> = subset.iter().copied().collect();

        // Own the follow-up pool (copy out of TLS scratch before nested 2-ply).
        let pool: Vec<Word> = {
            let mut buf = CandidateBuffer::new();
            followup_guess_pool(
                word_lists,
                subset,
                &extended_history,
                followup_turns,
                easy_mode,
                &mut buf,
            )
            .to_vec()
        };

        let mut follow_scores: Vec<GuessScore> = pool
            .iter()
            .copied()
            .map(|g| score_one_ply(word_lists, g, subset, &subset_set))
            .collect();
        follow_scores.sort_by(|a, b| compare_followup(*b, *a, followup_turns, subset.len()));
        follow_scores.truncate(follow_cap);

        let mut best_eg = f64::INFINITY;
        let mut best_ent = 0.0;
        for fs in follow_scores {
            let refined = score_two_ply_with_mode(
                word_lists,
                fs,
                subset,
                &subset_set,
                &extended_history,
                followup_turns,
                easy_mode,
            );
            if refined.expected_guesses < best_eg {
                best_eg = refined.expected_guesses;
                best_ent = refined.two_ply_entropy.max(refined.one_ply_entropy);
            }
        }
        if !best_eg.is_finite() {
            let fb =
                best_followup_one_ply(word_lists, subset, &pool, &subset_set, followup_turns);
            best_eg = fb.expected_guesses;
            best_ent = fb.entropy;
        }
        accumulated_further += weight * best_eg;
        accumulated_entropy += weight * best_ent;
    }

    let mut out = score;
    out.expected_guesses = 1.0 + accumulated_further;
    out.two_ply_entropy = accumulated_entropy;
    out.refined = true;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::words::WordLists;

    fn w(s: &str) -> Word {
        Word::parse(s).unwrap()
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
            expected_guesses: UNREFINED_EXPECTED_GUESSES,
            refined: false,
        }
    }

    fn gs_refined(
        word: &str,
        two_ply: f64,
        one_ply: f64,
        worst: usize,
        expected: f64,
        is_answer: bool,
        expected_guesses: f64,
    ) -> GuessScore {
        GuessScore {
            word: w(word),
            two_ply_entropy: two_ply,
            one_ply_entropy: one_ply,
            worst_bucket: worst,
            expected_remaining: expected,
            is_possible_answer: is_answer,
            frequency: 0,
            expected_guesses,
            refined: true,
        }
    }

    #[test]
    fn prefers_possible_answer_on_one_ply_tie() {
        let answer = gs("crate", 0.0, 1.0, 2, 1.0, true);
        let probe = gs("slate", 0.0, 1.0, 2, 1.0, false);
        assert_eq!(
            compare_one_ply(answer, probe, 4),
            std::cmp::Ordering::Greater
        );
        assert_eq!(compare_one_ply(probe, answer, 4), std::cmp::Ordering::Less);
    }

    #[test]
    fn compare_one_ply_prefers_smaller_worst_bucket_on_entropy_tie() {
        let better = gs("crate", 0.0, 1.0, 1, 1.5, false);
        let worse = gs("slate", 0.0, 1.0, 3, 1.5, false);
        assert_eq!(
            compare_one_ply(better, worse, 100),
            std::cmp::Ordering::Greater
        );
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
    fn compare_final_trusts_refined_expected_guesses_over_one_ply_gap() {
        // Both refined: lower expected_guesses wins even when 1-ply favors the peer.
        let better = gs_refined("slate", 2.0, 1.0, 4, 3.0, false, 3.2);
        let worse = gs_refined("crane", 1.0, 2.5, 4, 3.0, true, 3.9);
        assert_eq!(
            compare_final(better, worse, None, 100),
            std::cmp::Ordering::Greater,
            "refined lower expected_guesses must win despite higher 1-ply on peer"
        );
    }

    #[test]
    fn compare_final_prefers_higher_two_ply_when_expected_guesses_tie() {
        let a = gs_refined("slate", 2.5, 1.5, 4, 3.0, false, 3.2);
        let b = gs_refined("crane", 1.0, 1.5, 4, 3.0, true, 3.2);
        assert_eq!(
            compare_final(a, b, None, 100),
            std::cmp::Ordering::Greater,
            "equal expected_guesses: higher two_ply_entropy wins"
        );
    }

    #[test]
    fn compare_final_expected_guesses_primary_when_both_refined() {
        let a = gs_refined("slate", 2.0, 1.5, 4, 3.0, false, 3.1);
        let b = gs_refined("crane", 2.5, 1.4, 4, 3.0, true, 3.4);
        assert_eq!(
            compare_final(a, b, None, 100),
            std::cmp::Ordering::Greater,
            "lower expected_guesses is primary among refined scores"
        );
    }

    #[test]
    fn compare_final_unrefined_uses_epsilon_two_ply() {
        // Neither refined: within epsilon, higher two_ply_entropy wins.
        let high_two = gs("slate", 2.0, 1.0, 4, 3.0, false);
        let low_two = gs("crane", 1.0, 1.05, 4, 3.0, true);
        assert_eq!(
            compare_final(high_two, low_two, None, 100),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compare_final_prefers_sufficient_partition_when_turns_tight() {
        let good = gs_refined("slate", 0.0, 1.5, 2, 2.0, false, 3.0);
        let bad = gs_refined("crane", 0.0, 2.0, 5, 2.0, true, 2.5);
        assert_eq!(
            compare_final(good, bad, Some(3), 8),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compare_final_both_sufficient_prefers_smaller_worst_bucket() {
        let smaller = gs_refined("slate", 0.0, 1.0, 2, 2.0, false, 3.0);
        let larger = gs_refined("crane", 0.0, 3.0, 3, 2.0, true, 2.5);
        assert_eq!(
            compare_final(smaller, larger, Some(4), 8),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn compare_final_partition_branch_active_at_four_not_five_turns() {
        let good = gs_refined("slate", 0.0, 1.0, 2, 2.0, false, 3.0);
        let bad = gs_refined("crane", 0.0, 3.0, 6, 2.0, true, 2.5);
        assert_eq!(
            compare_final(good, bad, Some(4), 8),
            std::cmp::Ordering::Greater,
            "partition branch active at 4 turns"
        );
        // At 5 turns partition branch off: among refined, lower expected_guesses wins.
        assert_eq!(
            compare_final(good, bad, Some(5), 100),
            std::cmp::Ordering::Less,
            "at 5 turns expected_guesses decides among refined"
        );
    }

    #[test]
    fn score_two_ply_sets_refined_and_finite_expected_guesses() {
        let lists = WordLists::load();
        let remaining = [w("crate"), w("grate"), w("trace")];
        let remaining_set = set(&["crate", "grate", "trace"]);
        let score = score_one_ply(&lists, w("slate"), &remaining, &remaining_set);
        assert!(!score.refined);
        let refined = score_two_ply(&lists, score, &remaining, &remaining_set, &[], None);
        assert!(refined.refined);
        assert!(refined.expected_guesses.is_finite());
        assert!(refined.expected_guesses >= 1.0);
        assert!(refined.two_ply_entropy > 0.0);
        assert_eq!(refined.one_ply_entropy, score.one_ply_entropy);
    }

    #[test]
    fn score_two_ply_zero_for_single_remaining() {
        let lists = WordLists::load();
        let remaining = [w("crate")];
        let remaining_set = set(&["crate"]);
        let score = score_one_ply(&lists, w("slate"), &remaining, &remaining_set);
        let refined = score_two_ply(&lists, score, &remaining, &remaining_set, &[], None);
        assert_eq!(refined.two_ply_entropy, 0.0);
        assert!(refined.refined);
        assert_eq!(refined.expected_guesses, 1.0);
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
            compare_final(slate, taint, Some(3), 8),
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
            .max_by(|a, b| compare_final(**a, **b, Some(3), 8))
            .expect("at least one guess");
        assert_eq!(best.word, w("slate"));
        assert_eq!(best.worst_bucket, 7);
    }

    #[test]
    fn remaining_mass_prefers_discriminative_letters() {
        let remaining = [w("bound"), w("found"), w("hound"), w("mound")];
        let mass = RemainingMass::from_remaining(&remaining);
        // Words that hit the varying first letter + shared suffix letters should score high.
        let probe = mass.score_word(w("bhmpx"));
        let weak = mass.score_word(w("zzzzz"));
        assert!(probe > weak);
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

    #[test]
    fn win_pattern_bucket_is_last() {
        use crate::core::pattern::{Pattern, Tile};
        let win = Pattern::new([Tile::Correct; 5]);
        assert_eq!(pattern_bucket_index(win), PATTERN_BUCKETS - 1);
    }
}

// temporary - will remove
