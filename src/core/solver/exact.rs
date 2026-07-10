//! Depth-limited exact minimax for tiny remaining sets.

use std::collections::HashSet;

use crate::core::feedback::compute_feedback;
use crate::core::hard_mode::satisfies_hard_mode;
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

use super::pool::{rank_offlist_probes, top_offlist_probes, CompliantPool, ProbePolicy};
use super::SolverContext;

/// Sentinel for "cannot force a win within the remaining turns".
pub(crate) const EXACT_INF: usize = usize::MAX / 4;

/// Depth-limited exact minimax over remaining answers plus strong offlist probes.
pub(crate) fn exact_endgame_pick(ctx: &SolverContext<'_>, turns_left: usize) -> Option<Word> {
    let candidates = exact_candidate_pool(ctx, turns_left, /*probe_cap*/ 48);
    if candidates.is_empty() {
        return None;
    }

    // Prefer partitioning when more answers remain than turns (must often probe offlist).
    let prefer_probe = ctx.remaining.len() > turns_left;

    let mut best: Option<(Word, usize, usize, bool)> = None;
    for &guess in &candidates {
        let worst = exact_worst_case(
            ctx.word_lists,
            guess,
            ctx.remaining,
            ctx.history,
            turns_left,
            ctx.easy_mode,
            /*depth*/ 0,
        );
        let max_b = max_bucket_size(ctx.word_lists, guess, ctx.remaining);
        let is_answer = ctx.remaining_set.contains(&guess);
        let candidate = (guess, worst, max_b, is_answer);
        best = Some(match best {
            None => candidate,
            Some(prev) => {
                let better = if candidate.1 != prev.1 {
                    candidate.1 < prev.1
                } else if candidate.2 != prev.2 {
                    // Same force-win depth (or both INF): smaller worst bucket wins.
                    candidate.2 < prev.2
                } else if candidate.3 != prev.3 {
                    // When we must split a large set, prefer probes; otherwise prefer answers.
                    if prefer_probe {
                        !candidate.3 && prev.3
                    } else {
                        candidate.3 && !prev.3
                    }
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

    let (word, worst, max_b, _) = best?;
    // Accept only if the pick either forces a win or strictly partitions remaining.
    if worst < EXACT_INF || max_b < ctx.remaining.len() {
        Some(word)
    } else {
        None
    }
}

/// Candidate guesses for exact search: compliant remaining + top offlist partition probes.
fn exact_candidate_pool(ctx: &SolverContext<'_>, turns_left: usize, probe_cap: usize) -> Vec<Word> {
    let mut candidates: Vec<Word> = ctx
        .remaining
        .iter()
        .copied()
        .filter(|&w| ctx.hard_mode_ok(w) && !ctx.tried.contains(&w))
        .collect();

    if ctx.remaining.len() > turns_left {
        for g in top_offlist_probes(ctx, probe_cap) {
            if !candidates.contains(&g) {
                candidates.push(g);
            }
        }
    }
    candidates
}

/// Worst-case guesses needed (including `guess`) to finish `remaining` with `turns_left`.
/// Returns [`EXACT_INF`] if a win cannot be forced.
fn exact_worst_case(
    word_lists: &WordLists,
    guess: Word,
    remaining: &[Word],
    history: &[(Word, Pattern)],
    turns_left: usize,
    easy_mode: bool,
    depth: usize,
) -> usize {
    if turns_left == 0 {
        return EXACT_INF;
    }
    if remaining.is_empty() {
        return 0;
    }
    if remaining.len() == 1 {
        let only = remaining[0];
        if only == guess || easy_mode || satisfies_hard_mode(only, history) {
            // Playing `guess`: win if it is the answer, else one more guess for `only`.
            if only == guess {
                return 1;
            }
            // Wrong guess with 1 left — need another turn for the real answer.
            return if turns_left >= 2 { 2 } else { EXACT_INF };
        }
        return EXACT_INF;
    }

    // Cap recursion: deeper than a few plies uses bucket-size lower bound only.
    const MAX_EXACT_DEPTH: usize = 3;

    let mut parts: Vec<Vec<Word>> = vec![Vec::new(); 243];
    for &answer in remaining {
        let idx = word_lists.pattern_cache.bucket_or_compute(guess, answer);
        parts[idx].push(answer);
    }

    let mut worst = 1usize;
    for subset in &parts {
        if subset.is_empty() {
            continue;
        }
        // Correct-guess win bucket.
        if subset.len() == 1 && subset[0] == guess {
            worst = worst.max(1);
            continue;
        }
        if turns_left == 1 {
            // No turns left after this guess → lose unless it was a pure win (handled above).
            return EXACT_INF;
        }

        let pattern = compute_feedback(guess, subset[0]);
        let mut next_history = history.to_vec();
        next_history.push((guess, pattern));

        let branch = if depth >= MAX_EXACT_DEPTH {
            // Lower bound: need at least ceil(log) style — use max remaining after best answer guess.
            exact_branch_lower_bound(word_lists, subset, &next_history, turns_left - 1, easy_mode)
        } else {
            exact_best_reply(
                word_lists,
                subset,
                &next_history,
                turns_left - 1,
                easy_mode,
                depth + 1,
            )
        };

        if branch >= EXACT_INF {
            return EXACT_INF;
        }
        worst = worst.max(1 + branch);
    }
    worst
}

/// Best forced-win cost for a subset: try remaining answers first, then offlist probes when needed.
fn exact_best_reply(
    word_lists: &WordLists,
    subset: &[Word],
    history: &[(Word, Pattern)],
    turns_left: usize,
    easy_mode: bool,
    depth: usize,
) -> usize {
    if subset.is_empty() {
        return 0;
    }
    if turns_left == 0 {
        return EXACT_INF;
    }
    if subset.len() == 1 {
        let w = subset[0];
        if easy_mode || satisfies_hard_mode(w, history) {
            return 1;
        }
        return EXACT_INF;
    }

    let mut best = EXACT_INF;
    let mut replies: Vec<Word> = subset
        .iter()
        .copied()
        .filter(|&w| easy_mode || satisfies_hard_mode(w, history))
        .collect();

    // When more answers remain than turns, offlist probes are required to split clusters.
    if subset.len() > turns_left {
        let remaining_set: HashSet<Word> = subset.iter().copied().collect();
        let tried: HashSet<Word> = history.iter().map(|(g, _)| *g).collect();
        let pool = CompliantPool::for_turn(word_lists, history, easy_mode);
        for g in rank_offlist_probes(
            word_lists,
            pool.as_slice(),
            subset,
            &remaining_set,
            &tried,
            ProbePolicy::WorstBucket { cap: 12 },
        ) {
            if !replies.contains(&g) {
                replies.push(g);
            }
        }
    }

    for &reply in &replies {
        let cost = exact_worst_case(
            word_lists, reply, subset, history, turns_left, easy_mode, depth,
        );
        best = best.min(cost);
        if best <= 1 {
            break;
        }
    }
    best
}

/// Cheap lower bound when recursion is capped: 1 + worst bucket after best remaining-answer guess.
fn exact_branch_lower_bound(
    word_lists: &WordLists,
    subset: &[Word],
    history: &[(Word, Pattern)],
    turns_left: usize,
    easy_mode: bool,
) -> usize {
    if subset.len() <= 1 {
        return if subset.is_empty() { 0 } else { 1 };
    }
    if turns_left == 0 {
        return EXACT_INF;
    }

    let mut best_max = subset.len();
    for &guess in subset
        .iter()
        .filter(|&&w| easy_mode || satisfies_hard_mode(w, history))
    {
        let max_b = max_bucket_size(word_lists, guess, subset);
        best_max = best_max.min(max_b);
    }
    // If even the best answer-guess leaves a bucket that needs more than turns_left-1 pure
    // eliminations, treat as unsolvable at this bound (conservative for answer-only).
    if best_max >= subset.len() {
        return EXACT_INF;
    }
    // Need at least best_max more guesses in the worst leaf if we only pick answers one-by-one.
    if best_max > turns_left.saturating_sub(1).max(1) && subset.len() > turns_left {
        // Still might be solvable with probes; return a soft lower bound (not INF) so offlist
        // top-level candidates can still win on max_bucket comparison at the root.
        return 1 + best_max;
    }
    1 + best_max.min(turns_left)
}

pub(crate) fn max_bucket_size(word_lists: &WordLists, guess: Word, remaining: &[Word]) -> usize {
    let buckets = word_lists.pattern_cache.build_buckets_for(guess, remaining);
    buckets.counts.iter().copied().max().unwrap_or(0)
}
