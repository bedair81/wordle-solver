//! Main suggestion orchestration: heuristics → 1-ply → batched 2-ply → optional 3-ply.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::core::config::solver_config;
use crate::core::hard_mode::satisfies_hard_mode;
use crate::core::word::Word;

use super::candidates::{
    select_guess_candidates, should_run_three_ply, two_ply_interactive_cap,
    two_ply_non_interactive_cap, CandidateBuffer,
};
use super::heuristics::try_heuristic_pick;
use super::score::{
    compare_final, compare_one_ply, score_one_ply, score_three_ply_with_mode,
    score_two_ply_with_mode, GuessScore,
};
use super::second_guess::lookup_second_guess;
use super::{SecondGuessMode, SolverContext, Suggestion, SuggestionRequest};

thread_local! {
    static CANDIDATE_SCRATCH: std::cell::RefCell<CandidateBuffer> =
        std::cell::RefCell::new(CandidateBuffer::new());
}

pub(crate) fn compute_suggestion(req: SuggestionRequest<'_>) -> Option<Suggestion> {
    let SuggestionRequest {
        word_lists,
        remaining,
        history,
        turns_left,
        interactive,
        easy_mode,
        opening,
        second_guess,
    } = req;

    if remaining.len() == 1 {
        let word = remaining[0];
        if easy_mode || satisfies_hard_mode(word, history) {
            return Some(Suggestion {
                word,
                entropy: 0.0,
                expected_remaining: 1.0,
            });
        }
        return None;
    }

    // Precomputed second guess after the configured opener (O(1); consistent quality + snappy UI).
    if second_guess == SecondGuessMode::UseTable && !easy_mode {
        if let Some(word) = lookup_second_guess(history, opening) {
            if satisfies_hard_mode(word, history) && !history.iter().any(|(g, _)| *g == word) {
                let remaining_set: HashSet<Word> = remaining.iter().copied().collect();
                let score = score_one_ply(word_lists, word, remaining, &remaining_set);
                return Some(Suggestion {
                    word,
                    entropy: score.one_ply_entropy,
                    expected_remaining: score.expected_remaining,
                });
            }
        }
    }

    let ctx = SolverContext::new(word_lists, remaining, history, turns_left, easy_mode);

    if let Some(word) = try_heuristic_pick(&ctx) {
        return Some(ctx.suggestion_from_score(word));
    }

    refine_multi_ply(&ctx, interactive)
}

fn refine_multi_ply(ctx: &SolverContext<'_>, interactive: bool) -> Option<Suggestion> {
    CANDIDATE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let cfg = solver_config();
        let budget = cfg.interactive_budget();
        let reserve = Duration::from_millis(cfg.interactive_budget_reserve_ms);
        let budget_start = interactive.then(Instant::now);
        let budget_left = |start: Instant| {
            budget
                .checked_sub(start.elapsed())
                .unwrap_or(Duration::ZERO)
                > reserve
        };

        let guess_candidates = select_guess_candidates(
            ctx.word_lists,
            ctx.remaining,
            ctx.history,
            ctx.turns_left,
            interactive,
            ctx.easy_mode,
            &mut scratch,
        );

        if guess_candidates.is_empty() {
            return None;
        }

        let candidate_words: Vec<Word> = guess_candidates.to_vec();
        let cached_one_ply = std::mem::take(&mut scratch.precomputed_one_ply);

        // Parallel 1-ply scoring (each call is independent; pattern cache is shared read-only).
        let one_ply_scores: Vec<GuessScore> = candidate_words
            .par_iter()
            .map(|&guess| {
                if let Some(score) = cached_one_ply.get(&guess) {
                    *score
                } else {
                    score_one_ply(ctx.word_lists, guess, ctx.remaining, &ctx.remaining_set)
                }
            })
            .collect();

        if one_ply_scores.is_empty() {
            return None;
        }

        let remaining_len = ctx.remaining.len();
        let max_refine = if interactive {
            two_ply_interactive_cap(remaining_len, ctx.turns_left, one_ply_scores.len())
        } else {
            two_ply_non_interactive_cap(remaining_len, ctx.turns_left, one_ply_scores.len())
        };

        let mut sorted_indices: Vec<usize> = (0..one_ply_scores.len()).collect();
        sorted_indices
            .sort_by(|&a, &b| compare_one_ply(one_ply_scores[b], one_ply_scores[a], remaining_len));

        let refine_indices: Vec<usize> = sorted_indices.into_iter().take(max_refine).collect();
        let batch = if interactive {
            cfg.adaptive_two_ply_batch.max(1)
        } else {
            refine_indices.len().max(1)
        };

        let mut refined_scores: Vec<GuessScore> = Vec::with_capacity(refine_indices.len());
        let mut offset = 0usize;
        while offset < refine_indices.len() {
            if budget_start.is_some_and(|t| !budget_left(t)) {
                break;
            }
            let end = (offset + batch).min(refine_indices.len());
            let chunk: Vec<GuessScore> = refine_indices[offset..end]
                .par_iter()
                .map(|&idx| {
                    score_two_ply_with_mode(
                        ctx.word_lists,
                        one_ply_scores[idx],
                        ctx.remaining,
                        &ctx.remaining_set,
                        ctx.history,
                        ctx.turns_left,
                        ctx.easy_mode,
                    )
                })
                .collect();
            refined_scores.extend(chunk);
            offset = end;
            // Non-interactive: one batch already took everything.
            if !interactive {
                break;
            }
        }

        // Prefer refined multi-ply scores when available (they carry expected_guesses).
        let mut best = if !refined_scores.is_empty() {
            refined_scores
                .iter()
                .copied()
                .max_by(|a, b| compare_final(*a, *b, ctx.turns_left, remaining_len))?
        } else {
            one_ply_scores
                .iter()
                .copied()
                .max_by(|a, b| compare_final(*a, *b, ctx.turns_left, remaining_len))?
        };

        // Selective shallow 3-ply on hard mid-game states (bounded top-K, budget-aware).
        if should_run_three_ply(remaining_len, ctx.turns_left)
            && budget_start.map(budget_left).unwrap_or(true)
        {
            let k = cfg.three_ply_top_k.min(refined_scores.len()).max(1);
            let mut top: Vec<GuessScore> = refined_scores.clone();
            top.sort_by(|a, b| compare_final(*b, *a, ctx.turns_left, remaining_len));
            top.truncate(k);
            let deeper: Vec<GuessScore> = top
                .par_iter()
                .map(|s| {
                    score_three_ply_with_mode(
                        ctx.word_lists,
                        *s,
                        ctx.remaining,
                        ctx.history,
                        ctx.turns_left,
                        ctx.easy_mode,
                    )
                })
                .collect();
            for score in deeper {
                if compare_final(score, best, ctx.turns_left, remaining_len)
                    == std::cmp::Ordering::Greater
                {
                    best = score;
                }
            }
        }

        Some(Suggestion {
            word: best.word,
            entropy: if best.refined {
                best.two_ply_entropy
            } else {
                best.one_ply_entropy
            },
            expected_remaining: best.expected_remaining,
        })
    })
}
