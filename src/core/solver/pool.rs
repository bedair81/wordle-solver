//! Shared hard-mode-compliant guess pools and offlist probe ranking.
//!
//! Heuristics and exact search must use these helpers instead of re-implementing
//! `filter_hard_mode_compliant` / bucket ranking locally.

use std::collections::HashSet;

use crate::core::hard_mode::filter_hard_mode_compliant;
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

use super::score::score_one_ply;
use super::SolverContext;

/// Hard-mode-compliant guess pool for the current turn.
///
/// Borrows the full guess pool when easy mode or history is empty; otherwise owns
/// the filtered list (avoids cloning the full pool on the easy/opening path).
pub(crate) enum CompliantPool<'a> {
    Borrowed(&'a [Word]),
    Owned(Vec<Word>),
}

impl<'a> CompliantPool<'a> {
    pub(crate) fn for_turn(
        word_lists: &'a WordLists,
        history: &[(Word, Pattern)],
        easy_mode: bool,
    ) -> Self {
        if easy_mode || history.is_empty() {
            Self::Borrowed(&word_lists.guess_pool)
        } else {
            Self::Owned(filter_hard_mode_compliant(&word_lists.guess_pool, history))
        }
    }

    pub(crate) fn as_slice(&self) -> &[Word] {
        match self {
            Self::Borrowed(words) => words,
            Self::Owned(words) => words,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}

/// Ranking policy for offlist partition probes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ProbePolicy {
    /// Smaller worst bucket, then more nonempty buckets (exact endgame probe shortlist).
    WorstBucketThenNonempty { cap: usize },
    /// Smaller worst bucket only (exact-search reply probes).
    WorstBucket { cap: usize },
    /// More nonempty buckets, then higher 1-ply entropy (`score_best_probe`).
    NonemptyThenEntropy,
}

/// Rank offlist probes from `pool` under `policy`. Filters tried + remaining answers.
pub(crate) fn rank_offlist_probes(
    word_lists: &WordLists,
    pool: &[Word],
    remaining: &[Word],
    remaining_set: &HashSet<Word>,
    tried: &HashSet<Word>,
    policy: ProbePolicy,
) -> Vec<Word> {
    match policy {
        ProbePolicy::WorstBucketThenNonempty { cap } => {
            let mut scored: Vec<(Word, usize, usize)> = pool
                .iter()
                .copied()
                .filter(|w| !remaining_set.contains(w) && !tried.contains(w))
                .map(|g| {
                    let buckets = word_lists.pattern_cache.build_buckets_for(g, remaining);
                    let max_b = buckets.counts.iter().copied().max().unwrap_or(0);
                    (g, max_b, buckets.nonempty)
                })
                .filter(|(_, max_b, nonempty)| *nonempty > 1 && *max_b < remaining.len())
                .collect();
            scored.sort_by(|a, b| {
                a.1.cmp(&b.1)
                    .then_with(|| b.2.cmp(&a.2))
                    .then_with(|| a.0.cmp(&b.0))
            });
            scored.into_iter().take(cap).map(|(g, _, _)| g).collect()
        }
        ProbePolicy::WorstBucket { cap } => {
            let mut scored: Vec<(Word, usize)> = pool
                .iter()
                .copied()
                .filter(|w| !remaining_set.contains(w) && !tried.contains(w))
                .map(|g| {
                    let max_b = word_lists
                        .pattern_cache
                        .build_buckets_for(g, remaining)
                        .counts
                        .iter()
                        .copied()
                        .max()
                        .unwrap_or(0);
                    (g, max_b)
                })
                .filter(|(_, max_b)| *max_b < remaining.len())
                .collect();
            scored.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            scored.into_iter().take(cap).map(|(g, _)| g).collect()
        }
        ProbePolicy::NonemptyThenEntropy => pool
            .iter()
            .copied()
            .filter(|word| !remaining_set.contains(word) && !tried.contains(word))
            .map(|guess| {
                let buckets = word_lists.pattern_cache.build_buckets_for(guess, remaining);
                let score = score_one_ply(word_lists, guess, remaining, remaining_set);
                (guess, buckets.nonempty, score.one_ply_entropy)
            })
            .max_by(|a, b| {
                a.1.cmp(&b.1)
                    .then_with(|| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
                    .then_with(|| a.0.cmp(&b.0))
            })
            .map(|(guess, _, _)| vec![guess])
            .unwrap_or_default(),
    }
}

pub(crate) fn top_offlist_probes(ctx: &SolverContext<'_>, cap: usize) -> Vec<Word> {
    let pool = CompliantPool::for_turn(ctx.word_lists, ctx.history, ctx.easy_mode);
    rank_offlist_probes(
        ctx.word_lists,
        pool.as_slice(),
        ctx.remaining,
        &ctx.remaining_set,
        &ctx.tried,
        ProbePolicy::WorstBucketThenNonempty { cap },
    )
}

pub(crate) fn best_offlist_partition_probe(ctx: &SolverContext<'_>) -> Option<Word> {
    let pool = CompliantPool::for_turn(ctx.word_lists, ctx.history, ctx.easy_mode);
    if !ctx.easy_mode && !ctx.history.is_empty() && pool.is_empty() {
        return None;
    }
    let guess = rank_offlist_probes(
        ctx.word_lists,
        pool.as_slice(),
        ctx.remaining,
        &ctx.remaining_set,
        &ctx.tried,
        ProbePolicy::NonemptyThenEntropy,
    )
    .into_iter()
    .next()?;
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

/// Compliant candidates for minimax / endgame picks.
pub(crate) fn compliant_candidates(
    ctx: &SolverContext<'_>,
    offlist_only: bool,
) -> Option<Vec<Word>> {
    let pool = CompliantPool::for_turn(ctx.word_lists, ctx.history, ctx.easy_mode);
    if !ctx.easy_mode && !ctx.history.is_empty() && pool.is_empty() {
        return None;
    }

    let mut candidates: Vec<Word> = pool
        .as_slice()
        .iter()
        .copied()
        .filter(|&w| !ctx.tried.contains(&w))
        .collect();

    if !offlist_only {
        let mut seen: HashSet<Word> = candidates.iter().copied().collect();
        for &word in ctx.remaining {
            if ctx.hard_mode_ok(word) && !ctx.tried.contains(&word) && seen.insert(word) {
                candidates.push(word);
            }
        }
    }

    candidates.retain(|w| !offlist_only || !ctx.remaining_set.contains(w));
    Some(candidates)
}
