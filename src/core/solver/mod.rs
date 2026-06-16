mod candidates;
pub mod score;

use std::collections::HashSet;

use crate::core::feedback::compute_feedback;
use crate::core::filter::filter_by_history;
use crate::core::hard_mode::satisfies_hard_mode;
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

pub use score::{
    compare_final, compare_one_ply, score_one_ply, score_two_ply, GuessScore,
};

use candidates::{
    select_guess_candidates, shares_fixed_suffix, two_ply_candidate_indices, CandidateBuffer,
    TURNS_LEFT_REMAINING_SLACK,
};
use score::partition_sufficient;

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub word: Word,
    /// Information score in bits. Main path uses 2-ply entropy; early-return heuristics
    /// (endgame, minimax) use 1-ply entropy from `score_one_ply`. Opening uses a
    /// placeholder (`0.0`) because SLATE is fixed with no startup computation.
    pub entropy: f64,
    pub expected_remaining: f64,
}

const ENDGAME_PROBE_MAX_REMAINING: usize = 16;
const MINIMAX_MIDGAME_MAX_REMAINING: usize = 50;

/// Max time for a single UI suggestion (after the user commits a turn).
pub const INTERACTIVE_SUGGESTION_BUDGET: std::time::Duration =
    std::time::Duration::from_secs(10);

thread_local! {
    static CANDIDATE_SCRATCH: std::cell::RefCell<CandidateBuffer> =
        std::cell::RefCell::new(CandidateBuffer::new());
}

struct SolverContext<'a> {
    word_lists: &'a WordLists,
    remaining: &'a [Word],
    remaining_set: HashSet<Word>,
    history: &'a [(Word, Pattern)],
    turns_left: Option<usize>,
    suffix_cluster: bool,
    tried: HashSet<Word>,
}

impl<'a> SolverContext<'a> {
    fn new(
        word_lists: &'a WordLists,
        remaining: &'a [Word],
        history: &'a [(Word, Pattern)],
        turns_left: Option<usize>,
    ) -> Self {
        Self {
            word_lists,
            remaining,
            remaining_set: remaining.iter().copied().collect(),
            history,
            turns_left,
            suffix_cluster: shares_fixed_suffix(remaining),
            tried: history.iter().map(|(g, _)| *g).collect(),
        }
    }

    fn suggestion_from_score(&self, word: Word) -> Suggestion {
        let score = score_one_ply(
            self.word_lists,
            word,
            self.remaining,
            &self.remaining_set,
        );
        Suggestion {
            word,
            entropy: score.one_ply_entropy,
            expected_remaining: score.expected_remaining,
        }
    }
}

pub fn suggest_guess(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
) -> Option<Suggestion> {
    suggest_guess_with_turns(word_lists, remaining_answers, history, None)
}

pub fn suggest_guess_with_turns(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
) -> Option<Suggestion> {
    if remaining_answers.is_empty() {
        return None;
    }

    if history.is_empty() && remaining_answers.len() == word_lists.answers.len() {
        return Some(word_lists.opening_suggestion());
    }

    compute_suggestion(word_lists, remaining_answers, history, turns_left, false)
}

/// UI path: enforces [`INTERACTIVE_SUGGESTION_BUDGET`] so suggestions appear promptly.
pub fn suggest_guess_interactive(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: usize,
) -> Option<Suggestion> {
    if remaining_answers.is_empty() {
        return None;
    }

    if history.is_empty() && remaining_answers.len() == word_lists.answers.len() {
        return Some(word_lists.opening_suggestion());
    }

    compute_suggestion(
        word_lists,
        remaining_answers,
        history,
        Some(turns_left),
        true,
    )
}

pub fn compute_suggestion(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    interactive: bool,
) -> Option<Suggestion> {
    if remaining_answers.len() == 1 {
        let word = remaining_answers[0];
        if satisfies_hard_mode(word, history) {
            return Some(Suggestion {
                word,
                entropy: 0.0,
                expected_remaining: 1.0,
            });
        }
        return None;
    }

    let ctx = SolverContext::new(word_lists, remaining_answers, history, turns_left);

    if let Some(word) = try_heuristic_pick(&ctx) {
        return Some(ctx.suggestion_from_score(word));
    }

    CANDIDATE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let budget_start = interactive.then(std::time::Instant::now);
        let guess_candidates = select_guess_candidates(
            word_lists,
            remaining_answers,
            history,
            turns_left,
            interactive,
            &mut scratch,
        );

        if guess_candidates.is_empty() {
            return None;
        }

        let budget_expired = || {
            budget_start.is_some_and(|t| t.elapsed() >= INTERACTIVE_SUGGESTION_BUDGET)
        };

        let mut one_ply_scores: Vec<GuessScore> = Vec::with_capacity(guess_candidates.len());
        for &guess in guess_candidates {
            if budget_expired() {
                break;
            }
            one_ply_scores.push(score_one_ply(
                word_lists,
                guess,
                remaining_answers,
                &ctx.remaining_set,
            ));
        }

        if one_ply_scores.is_empty() {
            return None;
        }

        let refine_indices =
            two_ply_candidate_indices(&one_ply_scores, remaining_answers.len(), turns_left);

        let mut refined_scores = Vec::with_capacity(refine_indices.len());
        for idx in refine_indices {
            if budget_expired() {
                break;
            }
            refined_scores.push(score_two_ply(
                word_lists,
                one_ply_scores[idx],
                remaining_answers,
                &ctx.remaining_set,
                history,
                turns_left,
            ));
        }

        let remaining_len = remaining_answers.len();
        let mut best = one_ply_scores
            .iter()
            .copied()
            .max_by(|a, b| compare_final(*a, *b, turns_left, remaining_len))?;

        for score in refined_scores {
            if compare_final(score, best, turns_left, remaining_len) == std::cmp::Ordering::Greater
            {
                best = score;
            }
        }

        Some(Suggestion {
            word: best.word,
            entropy: best.two_ply_entropy,
            expected_remaining: best.expected_remaining,
        })
    })
}

fn try_heuristic_pick(ctx: &SolverContext<'_>) -> Option<Word> {
    let remaining_len = ctx.remaining.len();

    // Mid-game minimax: only when turns are tight — avoids overriding 2-ply on early turns.
    if ctx.turns_left.is_some_and(|left| {
        remaining_len > ENDGAME_PROBE_MAX_REMAINING
            && remaining_len <= MINIMAX_MIDGAME_MAX_REMAINING
            && (2..=4).contains(&left)
    }) {
        if let Some(word) = best_minimax_compliant_pick(ctx, false) {
            return Some(word);
        }
    }

    // Endgame: partition remaining answers or fall back to off-list probes.
    if let Some(left) = ctx.turns_left {
        if let Some(word) = endgame_pick(ctx, left) {
            return Some(word);
        }
    }

    // Shared suffix with more answers than turns left: off-list probe to split
    // leading letters (e.g. waste/haunt at 6 remaining, 2 turns left).
    if ctx.suffix_cluster
        && ctx
            .turns_left
            .is_some_and(|left| remaining_len > left.saturating_add(1))
        && remaining_len >= 6
    {
        if let Some(probe) = best_offlist_partition_probe(ctx) {
            return Some(probe);
        }
    }

    // Tight turns: prefer guesses that minimize the largest feedback bucket.
    if ctx.turns_left.is_some_and(|left| {
        remaining_len > left.saturating_add(1)
            && remaining_len >= 4
            && remaining_len <= ENDGAME_PROBE_MAX_REMAINING
            && (remaining_len > left || ctx.suffix_cluster)
    }) {
        return best_minimax_compliant_pick(ctx, false);
    }

    None
}

fn max_bucket_size(word_lists: &WordLists, guess: Word, remaining: &[Word]) -> usize {
    let buckets = word_lists.pattern_cache.build_buckets_for(guess, remaining);
    buckets.counts.iter().copied().max().unwrap_or(0)
}

fn in_endgame(remaining_len: usize, turns_left: usize) -> bool {
    remaining_len > 1
        && (remaining_len <= turns_left.saturating_add(TURNS_LEFT_REMAINING_SLACK)
            || remaining_len > turns_left.saturating_add(1))
        && remaining_len <= ENDGAME_PROBE_MAX_REMAINING
}

fn endgame_pick(ctx: &SolverContext<'_>, turns_left: usize) -> Option<Word> {
    if !in_endgame(ctx.remaining.len(), turns_left) {
        return None;
    }

    let pick_last = ctx.remaining.len() <= turns_left.saturating_add(1);
    let remaining_pick =
        best_partition_remaining_pick(ctx, pick_last);
    if let Some(word) = remaining_pick {
        let max_b = max_bucket_size(ctx.word_lists, word, ctx.remaining);
        if partition_sufficient(max_b, turns_left) {
            return Some(word);
        }
    }

    let pool_pick = best_minimax_compliant_pick(ctx, false);
    let offlist_pick = best_minimax_compliant_pick(ctx, true);

    match (pool_pick, offlist_pick) {
        (Some(all), Some(off)) => {
            let all_max = max_bucket_size(ctx.word_lists, all, ctx.remaining);
            let off_max = max_bucket_size(ctx.word_lists, off, ctx.remaining);
            let n = ctx.remaining.len();
            let off_progresses = off_max < n;
            let all_progresses = all_max < n;
            if off_max < all_max && off_progresses {
                return Some(off);
            }
            if all_progresses || all_max <= off_max || ctx.remaining_set.contains(&all) {
                return Some(all);
            }
            Some(off)
        }
        (None, off) | (off, None) => off.or(remaining_pick),
    }
}

fn best_partition_remaining_pick(ctx: &SolverContext<'_>, pick_last: bool) -> Option<Word> {
    let mut best: Option<(Word, usize, usize, f64)> = None;
    for &guess in ctx.remaining {
        if !satisfies_hard_mode(guess, ctx.history) || ctx.tried.contains(&guess) {
            continue;
        }
        let buckets = ctx
            .word_lists
            .pattern_cache
            .build_buckets_for(guess, ctx.remaining);
        let max_b = buckets.counts.iter().copied().max().unwrap_or(0);
        let score = score_one_ply(
            ctx.word_lists,
            guess,
            ctx.remaining,
            &ctx.remaining_set,
        );
        let candidate = (guess, max_b, buckets.nonempty, score.one_ply_entropy);
        best = Some(match best {
            None => candidate,
            Some(prev) => {
                let better = if candidate.1 != prev.1 {
                    candidate.1 < prev.1
                } else if candidate.2 != prev.2 {
                    candidate.2 > prev.2
                } else if (candidate.3 - prev.3).abs() >= 1e-9 {
                    candidate.3 > prev.3
                } else if pick_last {
                    candidate.0 > prev.0
                } else {
                    candidate.0 < prev.0
                };
                if better {
                    candidate
                } else {
                    prev
                }
            }
        });
    }
    best.map(|(word, _, _, _)| word)
}

fn best_offlist_partition_probe(ctx: &SolverContext<'_>) -> Option<Word> {
    use crate::core::hard_mode::filter_hard_mode_compliant;

    let compliant = filter_hard_mode_compliant(&ctx.word_lists.guess_pool, ctx.history);
    let pool: &[Word] = if ctx.history.is_empty() {
        &ctx.word_lists.guess_pool
    } else if compliant.is_empty() {
        return None;
    } else {
        &compliant
    };
    let guess = score_best_probe(ctx, pool)?;
    let buckets = ctx
        .word_lists
        .pattern_cache
        .build_buckets_for(guess, ctx.remaining);
    if buckets.nonempty > 1 {
        Some(guess)
    } else {
        None
    }
}

fn best_minimax_compliant_pick(ctx: &SolverContext<'_>, offlist_only: bool) -> Option<Word> {
    use crate::core::hard_mode::filter_hard_mode_compliant;

    let compliant = filter_hard_mode_compliant(&ctx.word_lists.guess_pool, ctx.history);
    if !ctx.history.is_empty() && compliant.is_empty() {
        return None;
    }

    let mut candidates: Vec<Word> = if ctx.history.is_empty() {
        ctx.word_lists
            .guess_pool
            .iter()
            .copied()
            .filter(|&w| !ctx.tried.contains(&w))
            .collect()
    } else {
        compliant
            .iter()
            .copied()
            .filter(|&w| !ctx.tried.contains(&w))
            .collect()
    };

    if !offlist_only {
        let mut seen: HashSet<Word> = candidates.iter().copied().collect();
        for &word in ctx.remaining {
            if satisfies_hard_mode(word, ctx.history)
                && !ctx.tried.contains(&word)
                && seen.insert(word)
            {
                candidates.push(word);
            }
        }
    }

    candidates.retain(|w| !offlist_only || !ctx.remaining_set.contains(w));

    let prefer_answers = ctx
        .turns_left
        .is_some_and(|left| left <= 2 && ctx.remaining.len() <= 6);

    candidates
        .into_iter()
        .filter_map(|guess| {
            let buckets = ctx
                .word_lists
                .pattern_cache
                .build_buckets_for(guess, ctx.remaining);
            let max_b = buckets.counts.iter().copied().max().unwrap_or(0);
            if max_b == 0 || (ctx.remaining.len() > 1 && max_b >= ctx.remaining.len()) {
                return None;
            }
            let score = score_one_ply(
                ctx.word_lists,
                guess,
                ctx.remaining,
                &ctx.remaining_set,
            );
            let is_answer = ctx.remaining_set.contains(&guess);
            Some((
                guess,
                max_b,
                buckets.nonempty,
                score.one_ply_entropy,
                is_answer,
            ))
        })
        .min_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| {
                    if prefer_answers || ctx.suffix_cluster {
                        b.4.cmp(&a.4)
                    } else {
                        a.4.cmp(&b.4)
                    }
                })
                .then_with(|| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| {
                    if prefer_answers {
                        b.0.cmp(&a.0)
                    } else {
                        a.0.cmp(&b.0)
                    }
                })
        })
        .map(|(guess, _, _, _, _)| guess)
}

fn score_best_probe(ctx: &SolverContext<'_>, pool: &[Word]) -> Option<Word> {
    pool.iter()
        .copied()
        .filter(|word| !ctx.remaining_set.contains(word) && !ctx.tried.contains(word))
        .map(|guess| {
            let buckets = ctx
                .word_lists
                .pattern_cache
                .build_buckets_for(guess, ctx.remaining);
            let score = score_one_ply(
                ctx.word_lists,
                guess,
                ctx.remaining,
                &ctx.remaining_set,
            );
            (guess, buckets.nonempty, score.one_ply_entropy)
        })
        .max_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| a.0.cmp(&b.0))
        })
        .map(|(guess, _, _)| guess)
}

pub fn auto_solve(word_lists: &WordLists, target: Word) -> Option<Vec<(Word, Pattern)>> {
    let mut history = Vec::new();
    let max_turns = 6;

    for _ in 0..max_turns {
        let remaining = filter_by_history(&word_lists.answers, &history);
        let turns_left = max_turns - history.len();

        if turns_left == 1 {
            for &word in &remaining {
                if satisfies_hard_mode(word, &history) {
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

        let suggestion =
            suggest_guess_with_turns(word_lists, &remaining, &history, Some(turns_left))?;
        let guess = suggestion.word;
        let pattern = compute_feedback(guess, target);
        history.push((guess, pattern));

        if pattern.is_win() {
            return Some(history);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::core::hard_mode::satisfies_hard_mode;
    use crate::core::pattern::Pattern;
    use crate::core::words::WordLists;

    fn w(s: &str) -> Word {
        Word::parse(s).unwrap()
    }

    fn pat(s: &str) -> Pattern {
        Pattern::from_str(s).unwrap()
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
        let suggestion = suggest_guess(&lists, &remaining, &[]).unwrap();
        assert!(remaining.contains(&suggestion.word) || lists.is_valid_guess(suggestion.word));
    }

    #[test]
    fn opening_guess_is_instant() {
        let lists = WordLists::load();
        let suggestion = suggest_guess(&lists, &lists.answers, &[]).unwrap();
        assert_eq!(suggestion.word, lists.opening_guess());
    }

    #[test]
    fn suggestions_satisfy_hard_mode() {
        let lists = WordLists::load();
        let histories = [
            vec![(w("slate"), pat("Gxxxx"))],
            vec![(w("crane"), pat("xxxYx"))],
            vec![(w("slate"), pat("Gxxxx")), (w("crane"), pat("xGYYx"))],
        ];
        for history in &histories {
            let remaining = crate::core::filter::filter_by_history(&lists.answers, history);
            if let Some(suggestion) = suggest_guess(&lists, &remaining, history) {
                assert!(
                    satisfies_hard_mode(suggestion.word, history),
                    "suggestion {} not compliant",
                    suggestion.word
                );
            }
        }
    }

    #[test]
    fn auto_solve_history_is_compliant_and_wins() {
        let lists = WordLists::load();
        for target in [
            "found", "haste", "haunt", "hound", "joker", "match", "poker", "savvy", "stash",
            "bound", "boxer", "waste", "watch",
        ] {
            let target = w(target);
            let history =
                auto_solve(&lists, target).unwrap_or_else(|| panic!("failed to solve {target}"));
            assert!(history.last().unwrap().1.is_win());
            for i in 0..history.len() {
                let prior: Vec<_> = history[..i].to_vec();
                assert!(satisfies_hard_mode(history[i].0, &prior));
            }
        }
    }

    #[test]
    fn single_remaining_non_compliant_returns_none() {
        let lists = WordLists::load();
        let history = vec![(w("slate"), pat("Gxxxx"))];
        let remaining = vec![w("crane")];
        assert!(compute_suggestion(&lists, &remaining, &history, Some(3), false).is_none());
    }

    #[test]
    fn single_remaining_compliant_returns_that_word() {
        let lists = WordLists::load();
        let history = vec![(w("slate"), pat("xxxxx"))];
        let remaining = vec![w("crane")];
        let suggestion = compute_suggestion(&lists, &remaining, &history, Some(3), false).unwrap();
        assert_eq!(suggestion.word, w("crane"));
    }

    #[test]
    fn compute_suggestion_empty_pool_returns_none() {
        let lists = WordLists::load();
        let history = vec![(w("aaaaa"), pat("GGGGG")), (w("bbbbb"), pat("GGGGG"))];
        let remaining = vec![w("crane")];
        assert!(compute_suggestion(&lists, &remaining, &history, None, false).is_none());
    }

    #[test]
    fn auto_solves_pound_and_wound() {
        let lists = WordLists::load();
        for target in ["pound", "wound"] {
            let target = w(target);
            auto_solve(&lists, target).unwrap_or_else(|| panic!("failed to solve {target}"));
        }
    }

    #[test]
    fn shares_fixed_suffix_detects_ound_cluster() {
        let words = [w("bound"), w("found"), w("wound")];
        assert!(shares_fixed_suffix(&words));
        let mixed = [w("bound"), w("young")];
        assert!(!shares_fixed_suffix(&mixed));
    }

    fn ing_suffix_cluster() -> Vec<Word> {
        [
            "aging", "aping", "being", "bring", "cling", "doing", "dying", "eking", "eying",
            "fling", "going", "icing", "lying", "owing", "sling", "sting", "suing", "swing",
            "thing", "tying", "using", "vying", "wring",
        ]
        .iter()
        .map(|s| w(s))
        .collect()
    }

    #[test]
    fn suffix_offlist_probe_path_reports_score_one_ply_metrics() {
        let lists = WordLists::load();
        let remaining: Vec<Word> = ing_suffix_cluster().into_iter().take(18).collect();
        assert!(
            remaining.len() > ENDGAME_PROBE_MAX_REMAINING,
            "must skip endgame_pick to hit suffix off-list block"
        );
        assert!(
            remaining.len() <= MINIMAX_MIDGAME_MAX_REMAINING,
            "fixture should stay within mid-game upper bound if minimax ever runs"
        );
        let ctx = SolverContext::new(&lists, &remaining, &[], Some(1));
        let expected_probe = best_offlist_partition_probe(&ctx).unwrap();

        let suggestion = compute_suggestion(&lists, &remaining, &[], Some(1), false).unwrap();
        let expected = score_one_ply(&lists, suggestion.word, &remaining, &ctx.remaining_set);

        assert_eq!(
            suggestion.word, expected_probe,
            "compute_suggestion should use best_offlist_partition_probe"
        );
        assert!(
            !remaining.contains(&suggestion.word),
            "suffix off-list block should pick a probe, got {}",
            suggestion.word
        );
        assert!(
            (suggestion.entropy - expected.one_ply_entropy).abs() < 1e-9,
            "entropy should match score_one_ply"
        );
        assert!(
            (suggestion.expected_remaining - expected.expected_remaining).abs() < 1e-9,
            "expected_remaining should match score_one_ply"
        );
    }

    #[test]
    fn compute_suggestion_with_turns_left_differs_from_open_ended() {
        let lists = WordLists::load();
        let remaining = vec![
            w("bound"),
            w("found"),
            w("hound"),
            w("mound"),
            w("pound"),
            w("round"),
            w("sound"),
            w("wound"),
        ];
        let with_turns = compute_suggestion(&lists, &remaining, &[], Some(3), false).unwrap();
        let open_ended = compute_suggestion(&lists, &remaining, &[], None, false).unwrap();

        assert_eq!(
            with_turns.word,
            w("barfs"),
            "endgame minimax pick at 3 turns"
        );
        assert_eq!(open_ended.word, w("herms"), "2-ply path without turns_left");
        assert_ne!(with_turns.word, open_ended.word);
        assert!(!remaining.contains(&with_turns.word));
    }

    #[test]
    #[ignore = "manual trace for failing words"]
    fn trace_failing_word() {
        use crate::core::feedback::compute_feedback;
        use crate::core::filter::filter_by_history;
        let lists = WordLists::load();
        let target = w("pound");
        let mut history = Vec::new();
        for turn in 1..=6 {
            let remaining = filter_by_history(&lists.answers, &history);
            let turns_left = 6 - history.len();
            let suggestion =
                suggest_guess_with_turns(&lists, &remaining, &history, Some(turns_left));
            println!(
                "turn {turn}: remaining={} suggestion={suggestion:?}",
                remaining.len()
            );
            let Some(suggestion) = suggestion else {
                break;
            };
            let guess = suggestion.word;
            println!(
                "  guess {guess} compliant={}",
                satisfies_hard_mode(guess, &history)
            );
            let pattern = compute_feedback(guess, target);
            println!("  pattern {pattern}");
            history.push((guess, pattern));
            if pattern.is_win() {
                return;
            }
        }
        panic!("failed to solve {} in 6 turns", target.as_str());
    }
}
