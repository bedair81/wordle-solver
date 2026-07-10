//! Early-exit heuristic strategies before the main multi-ply scoring path.

use crate::core::config::solver_config;
use crate::core::word::Word;

use super::exact::{exact_endgame_pick, max_bucket_size};
use super::pool::{best_offlist_partition_probe, compliant_candidates};
use super::score::{partition_sufficient, score_one_ply};
use super::SolverContext;

/// Ordered heuristic phases. Each runs at most once; first hit wins.
#[derive(Clone, Copy, Debug)]
enum HeuristicPhase {
    ExactEndgame,
    MidMinimax,
    Endgame,
    SuffixOfflist,
    TightMinimax,
}

fn applicable_phases(ctx: &SolverContext<'_>) -> Vec<HeuristicPhase> {
    let remaining_len = ctx.remaining.len();
    let cfg = solver_config();
    let mut phases = Vec::with_capacity(5);

    if ctx.turns_left.is_some_and(|left| {
        remaining_len > 1 && remaining_len <= cfg.exact_endgame_max_remaining && left > 0
    }) {
        phases.push(HeuristicPhase::ExactEndgame);
    }

    if ctx.turns_left.is_some_and(|left| {
        remaining_len > cfg.endgame_probe_max_remaining
            && remaining_len <= cfg.minimax_midgame_max_remaining
            && (2..=4).contains(&left)
    }) {
        phases.push(HeuristicPhase::MidMinimax);
    }

    if ctx.turns_left.is_some() {
        phases.push(HeuristicPhase::Endgame);
    }

    if ctx.suffix_cluster
        && ctx
            .turns_left
            .is_some_and(|left| remaining_len > left.saturating_add(1))
        && remaining_len >= 6
    {
        phases.push(HeuristicPhase::SuffixOfflist);
    }

    if ctx.turns_left.is_some_and(|left| {
        remaining_len > left.saturating_add(1)
            && (4..=cfg.endgame_probe_max_remaining).contains(&remaining_len)
            && (remaining_len > left || ctx.suffix_cluster)
    }) {
        phases.push(HeuristicPhase::TightMinimax);
    }

    phases
}

pub(crate) fn try_heuristic_pick(ctx: &SolverContext<'_>) -> Option<Word> {
    for phase in applicable_phases(ctx) {
        let pick = match phase {
            HeuristicPhase::ExactEndgame => {
                let left = ctx.turns_left?;
                exact_endgame_pick(ctx, left)
            }
            HeuristicPhase::MidMinimax | HeuristicPhase::TightMinimax => {
                best_minimax_compliant_pick(ctx, false)
            }
            HeuristicPhase::Endgame => {
                let left = ctx.turns_left?;
                endgame_pick(ctx, left)
            }
            HeuristicPhase::SuffixOfflist => best_offlist_partition_probe(ctx),
        };
        if let Some(word) = pick {
            return Some(word);
        }
    }
    None
}

/// True when remaining is small enough for endgame partition / probe logic.
///
/// Historically this OR'd two complementary inequalities that together covered every
/// `remaining > 1` case up to `endgame_probe_max_remaining`. Keep that effective
/// predicate explicitly so the policy is readable.
fn in_endgame(remaining_len: usize, _turns_left: usize) -> bool {
    let endgame_max = solver_config().endgame_probe_max_remaining;
    remaining_len > 1 && remaining_len <= endgame_max
}

fn endgame_pick(ctx: &SolverContext<'_>, turns_left: usize) -> Option<Word> {
    if !in_endgame(ctx.remaining.len(), turns_left) {
        return None;
    }

    let pick_last = ctx.remaining.len() <= turns_left.saturating_add(1);
    let remaining_pick = best_partition_remaining_pick(ctx, pick_last);
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
        if !ctx.hard_mode_ok(guess) || ctx.tried.contains(&guess) {
            continue;
        }
        let buckets = ctx
            .word_lists
            .pattern_cache
            .build_buckets_for(guess, ctx.remaining);
        let max_b = buckets.counts.iter().copied().max().unwrap_or(0);
        let score = score_one_ply(ctx.word_lists, guess, ctx.remaining, &ctx.remaining_set);
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

pub(crate) fn best_minimax_compliant_pick(
    ctx: &SolverContext<'_>,
    offlist_only: bool,
) -> Option<Word> {
    let candidates = compliant_candidates(ctx, offlist_only)?;

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
            let score = score_one_ply(ctx.word_lists, guess, ctx.remaining, &ctx.remaining_set);
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
