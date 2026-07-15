mod candidates;
pub mod score;
mod second_guess;

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rayon::prelude::*;

use crate::core::config::solver_config;
use crate::core::feedback::compute_feedback;
use crate::core::filter::{filter_by_history, remaining_solutions};
use crate::core::hard_mode::satisfies_hard_mode;
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::{WordLists, OPENING_GUESS};

pub use score::{
    compare_final, compare_one_ply, score_one_ply, score_two_ply, GuessScore, PATTERN_BUCKETS,
};
pub use second_guess::lookup_second_guess;

use candidates::{
    select_guess_candidates, shares_fixed_suffix, should_run_three_ply, two_ply_interactive_cap,
    two_ply_non_interactive_cap, CandidateBuffer,
};
use score::{partition_sufficient, score_three_ply_with_mode, score_two_ply_with_mode};

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub word: Word,
    /// Information score in bits. Main path uses 2-ply entropy; early-return heuristics
    /// (endgame, minimax) use 1-ply entropy from `score_one_ply`. Opening uses a
    /// placeholder (`0.0`) because the opener is fixed with no startup computation.
    pub entropy: f64,
    pub expected_remaining: f64,
}

/// Max time for a single UI suggestion (after the user commits a turn).
pub fn interactive_suggestion_budget() -> std::time::Duration {
    solver_config().interactive_budget()
}

/// Back-compat alias used by older call sites / tests.
pub const INTERACTIVE_SUGGESTION_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

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
    easy_mode: bool,
    suffix_cluster: bool,
    tried: HashSet<Word>,
}

impl<'a> SolverContext<'a> {
    fn new(
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

    fn suggestion_from_score(&self, word: Word) -> Suggestion {
        let score = score_one_ply(self.word_lists, word, self.remaining, &self.remaining_set);
        Suggestion {
            word,
            entropy: score.one_ply_entropy,
            expected_remaining: score.expected_remaining,
        }
    }

    fn hard_mode_ok(&self, word: Word) -> bool {
        self.easy_mode || satisfies_hard_mode(word, self.history)
    }
}

pub fn suggest_guess(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
) -> Option<Suggestion> {
    suggest_guess_with_options(
        word_lists,
        remaining_answers,
        history,
        None,
        false,
        false,
        OPENING_GUESS,
    )
}

pub fn suggest_guess_with_turns(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
) -> Option<Suggestion> {
    suggest_guess_with_options(
        word_lists,
        remaining_answers,
        history,
        turns_left,
        false,
        false,
        OPENING_GUESS,
    )
}

pub fn suggest_guess_with_options(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    interactive: bool,
    easy_mode: bool,
    opening: Word,
) -> Option<Suggestion> {
    // When the answer list is exhausted but feedback still matches guess-pool words
    // (NYT answer missing from answers.txt), keep solving against those matches.
    let pool_fallback;
    let remaining_answers = if remaining_answers.is_empty() {
        pool_fallback = remaining_solutions(word_lists, history);
        if pool_fallback.is_empty() {
            return None;
        }
        pool_fallback.as_slice()
    } else {
        remaining_answers
    };

    if history.is_empty() && remaining_answers.len() == word_lists.answers.len() {
        return Some(word_lists.opening_suggestion(opening));
    }

    compute_suggestion_with_mode_opening(
        word_lists,
        remaining_answers,
        history,
        turns_left,
        interactive,
        easy_mode,
        opening,
    )
}

/// UI path: enforces interactive budget so suggestions appear promptly.
pub fn suggest_guess_interactive(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: usize,
    easy_mode: bool,
    opening: Word,
) -> Option<Suggestion> {
    suggest_guess_with_options(
        word_lists,
        remaining_answers,
        history,
        Some(turns_left),
        true,
        easy_mode,
        opening,
    )
}

/// Async suggestion job: compute off the calling thread; poll with a generation counter.
pub struct SuggestionJob {
    generation: u64,
    rx: Receiver<(u64, Option<Suggestion>)>,
}

impl SuggestionJob {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Non-blocking poll. `None` means still running; `Some` is the finished result
    /// (only returned when `generation` still matches).
    pub fn try_recv(&self) -> Option<Option<Suggestion>> {
        match self.rx.try_recv() {
            Ok((gen, suggestion)) if gen == self.generation => Some(suggestion),
            Ok(_) => Some(None), // stale — treat as no suggestion
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(None),
        }
    }
}

/// Spawn a background suggestion computation. Caller should bump `generation` on undo/reset.
pub fn spawn_suggestion_job(
    word_lists: Arc<WordLists>,
    remaining: Vec<Word>,
    history: Vec<(Word, Pattern)>,
    turns_left: usize,
    easy_mode: bool,
    opening: Word,
    generation: u64,
) -> SuggestionJob {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = suggest_guess_interactive(
            &word_lists,
            &remaining,
            &history,
            turns_left,
            easy_mode,
            opening,
        );
        let _ = tx.send((generation, result));
    });
    SuggestionJob { generation, rx }
}

pub fn compute_suggestion(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    interactive: bool,
) -> Option<Suggestion> {
    compute_suggestion_with_mode(
        word_lists,
        remaining_answers,
        history,
        turns_left,
        interactive,
        false,
    )
}

pub fn compute_suggestion_with_mode(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    interactive: bool,
    easy_mode: bool,
) -> Option<Suggestion> {
    compute_suggestion_with_mode_opening(
        word_lists,
        remaining_answers,
        history,
        turns_left,
        interactive,
        easy_mode,
        OPENING_GUESS,
    )
}

/// Like [`compute_suggestion_with_mode_opening`] but never consults the second-guess table
/// (used when regenerating that table offline).
pub fn compute_suggestion_live(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    easy_mode: bool,
) -> Option<Suggestion> {
    compute_suggestion_with_mode_opening_ex(
        word_lists,
        remaining_answers,
        history,
        turns_left,
        false,
        easy_mode,
        OPENING_GUESS,
        false,
    )
}

pub fn compute_suggestion_with_mode_opening(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    interactive: bool,
    easy_mode: bool,
    opening: Word,
) -> Option<Suggestion> {
    compute_suggestion_with_mode_opening_ex(
        word_lists,
        remaining_answers,
        history,
        turns_left,
        interactive,
        easy_mode,
        opening,
        true,
    )
}

fn compute_suggestion_with_mode_opening_ex(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    interactive: bool,
    easy_mode: bool,
    opening: Word,
    use_second_guess_table: bool,
) -> Option<Suggestion> {
    if remaining_answers.len() == 1 {
        let word = remaining_answers[0];
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
    if use_second_guess_table && !easy_mode {
        if let Some(word) = lookup_second_guess(history, opening) {
            if satisfies_hard_mode(word, history) && !history.iter().any(|(g, _)| *g == word) {
                let remaining_set: HashSet<Word> = remaining_answers.iter().copied().collect();
                let score = score_one_ply(word_lists, word, remaining_answers, &remaining_set);
                return Some(Suggestion {
                    word,
                    entropy: score.one_ply_entropy,
                    expected_remaining: score.expected_remaining,
                });
            }
        }
    }

    let ctx = SolverContext::new(
        word_lists,
        remaining_answers,
        history,
        turns_left,
        easy_mode,
    );

    if let Some(word) = try_heuristic_pick(&ctx) {
        return Some(ctx.suggestion_from_score(word));
    }

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
            word_lists,
            remaining_answers,
            history,
            turns_left,
            interactive,
            easy_mode,
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
                    score_one_ply(word_lists, guess, remaining_answers, &ctx.remaining_set)
                }
            })
            .collect();

        if one_ply_scores.is_empty() {
            return None;
        }

        let remaining_len = remaining_answers.len();
        let max_refine = if interactive {
            two_ply_interactive_cap(remaining_len, turns_left, one_ply_scores.len())
        } else {
            two_ply_non_interactive_cap(remaining_len, turns_left, one_ply_scores.len())
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
                        word_lists,
                        one_ply_scores[idx],
                        remaining_answers,
                        &ctx.remaining_set,
                        history,
                        turns_left,
                        easy_mode,
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
                .max_by(|a, b| compare_final(*a, *b, turns_left, remaining_len))?
        } else {
            one_ply_scores
                .iter()
                .copied()
                .max_by(|a, b| compare_final(*a, *b, turns_left, remaining_len))?
        };

        // Selective shallow 3-ply on hard mid-game states (bounded top-K, budget-aware).
        if should_run_three_ply(remaining_len, turns_left)
            && budget_start.map(|t| budget_left(t)).unwrap_or(true)
        {
            let k = cfg.three_ply_top_k.min(refined_scores.len()).max(1);
            let mut top: Vec<GuessScore> = refined_scores.clone();
            top.sort_by(|a, b| compare_final(*b, *a, turns_left, remaining_len));
            top.truncate(k);
            let deeper: Vec<GuessScore> = top
                .par_iter()
                .map(|s| {
                    score_three_ply_with_mode(
                        word_lists,
                        *s,
                        remaining_answers,
                        history,
                        turns_left,
                        easy_mode,
                    )
                })
                .collect();
            for score in deeper {
                if compare_final(score, best, turns_left, remaining_len)
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

fn try_heuristic_pick(ctx: &SolverContext<'_>) -> Option<Word> {
    let remaining_len = ctx.remaining.len();
    let cfg = solver_config();

    // Exact endgame minimax for tiny remaining sets.
    if let Some(left) = ctx.turns_left {
        if remaining_len > 1 && remaining_len <= cfg.exact_endgame_max_remaining {
            if let Some(word) = exact_endgame_pick(ctx, left) {
                return Some(word);
            }
        }
    }

    // Mid-game minimax: only when turns are tight — avoids overriding 2-ply on early turns.
    if ctx.turns_left.is_some_and(|left| {
        remaining_len > cfg.endgame_probe_max_remaining
            && remaining_len <= cfg.minimax_midgame_max_remaining
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
            && (4..=cfg.endgame_probe_max_remaining).contains(&remaining_len)
            && (remaining_len > left || ctx.suffix_cluster)
    }) {
        return best_minimax_compliant_pick(ctx, false);
    }

    None
}

/// Sentinel for "cannot force a win within the remaining turns".
const EXACT_INF: usize = usize::MAX / 4;

/// Depth-limited exact minimax over remaining answers plus strong offlist probes.
fn exact_endgame_pick(ctx: &SolverContext<'_>, turns_left: usize) -> Option<Word> {
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

fn top_offlist_probes(ctx: &SolverContext<'_>, cap: usize) -> Vec<Word> {
    use crate::core::hard_mode::filter_hard_mode_compliant;

    let pool = if ctx.easy_mode || ctx.history.is_empty() {
        ctx.word_lists.guess_pool.clone()
    } else {
        filter_hard_mode_compliant(&ctx.word_lists.guess_pool, ctx.history)
    };

    let mut scored: Vec<(Word, usize, usize)> = pool
        .iter()
        .copied()
        .filter(|w| !ctx.remaining_set.contains(w) && !ctx.tried.contains(w))
        .map(|g| {
            let buckets = ctx
                .word_lists
                .pattern_cache
                .build_buckets_for(g, ctx.remaining);
            let max_b = buckets.counts.iter().copied().max().unwrap_or(0);
            (g, max_b, buckets.nonempty)
        })
        .filter(|(_, max_b, nonempty)| *nonempty > 1 && *max_b < ctx.remaining.len())
        .collect();

    // Prefer smaller worst bucket, then more nonempty buckets.
    scored.sort_by(|a, b| {
        a.1.cmp(&b.1)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.into_iter().take(cap).map(|(g, _, _)| g).collect()
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
        use crate::core::hard_mode::filter_hard_mode_compliant;
        let pool = if easy_mode || history.is_empty() {
            word_lists.guess_pool.clone()
        } else {
            filter_hard_mode_compliant(&word_lists.guess_pool, history)
        };
        let mut scored: Vec<(Word, usize)> = pool
            .iter()
            .copied()
            .filter(|w| !remaining_set.contains(w) && !tried.contains(w))
            .map(|g| {
                let max_b = word_lists
                    .pattern_cache
                    .build_buckets_for(g, subset)
                    .counts
                    .iter()
                    .copied()
                    .max()
                    .unwrap_or(0);
                (g, max_b)
            })
            .filter(|(_, max_b)| *max_b < subset.len())
            .collect();
        scored.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        for (g, _) in scored.into_iter().take(12) {
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

fn max_bucket_size(word_lists: &WordLists, guess: Word, remaining: &[Word]) -> usize {
    let buckets = word_lists.pattern_cache.build_buckets_for(guess, remaining);
    buckets.counts.iter().copied().max().unwrap_or(0)
}

fn in_endgame(remaining_len: usize, turns_left: usize) -> bool {
    let slack = solver_config().turns_left_remaining_slack;
    let endgame_max = solver_config().endgame_probe_max_remaining;
    remaining_len > 1
        && (remaining_len <= turns_left.saturating_add(slack)
            || remaining_len > turns_left.saturating_add(1))
        && remaining_len <= endgame_max
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

fn best_offlist_partition_probe(ctx: &SolverContext<'_>) -> Option<Word> {
    use crate::core::hard_mode::filter_hard_mode_compliant;

    let compliant = if ctx.easy_mode {
        ctx.word_lists.guess_pool.clone()
    } else {
        filter_hard_mode_compliant(&ctx.word_lists.guess_pool, ctx.history)
    };
    let pool: &[Word] = if ctx.history.is_empty() || ctx.easy_mode {
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

    let compliant = if ctx.easy_mode {
        ctx.word_lists.guess_pool.clone()
    } else {
        filter_hard_mode_compliant(&ctx.word_lists.guess_pool, ctx.history)
    };
    if !ctx.easy_mode && !ctx.history.is_empty() && compliant.is_empty() {
        return None;
    }

    let mut candidates: Vec<Word> = if ctx.history.is_empty() || ctx.easy_mode {
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
            if ctx.hard_mode_ok(word) && !ctx.tried.contains(&word) && seen.insert(word) {
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

fn score_best_probe(ctx: &SolverContext<'_>, pool: &[Word]) -> Option<Word> {
    pool.iter()
        .copied()
        .filter(|word| !ctx.remaining_set.contains(word) && !ctx.tried.contains(word))
        .map(|guess| {
            let buckets = ctx
                .word_lists
                .pattern_cache
                .build_buckets_for(guess, ctx.remaining);
            let score = score_one_ply(ctx.word_lists, guess, ctx.remaining, &ctx.remaining_set);
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
    auto_solve_with_options(word_lists, target, false, OPENING_GUESS)
}

pub fn auto_solve_with_options(
    word_lists: &WordLists,
    target: Word,
    easy_mode: bool,
    opening: Word,
) -> Option<Vec<(Word, Pattern)>> {
    let mut history = Vec::new();
    let max_turns = 6;

    for _ in 0..max_turns {
        let remaining = filter_by_history(&word_lists.answers, &history);
        let turns_left = max_turns - history.len();

        if turns_left == 1 {
            for &word in &remaining {
                if easy_mode || satisfies_hard_mode(word, &history) {
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

        let suggestion = suggest_guess_with_options(
            word_lists,
            &remaining,
            &history,
            Some(turns_left),
            false,
            easy_mode,
            opening,
        )?;
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
    use super::*;
    use crate::core::hard_mode::satisfies_hard_mode;
    use crate::core::pattern::Pattern;
    use crate::core::words::shared_word_lists;

    fn w(s: &str) -> Word {
        Word::parse(s).unwrap()
    }

    fn pat(s: &str) -> Pattern {
        Pattern::from_str(s).unwrap()
    }

    #[test]
    fn opening_guess_is_valid() {
        let lists = shared_word_lists();
        assert!(lists.is_valid_guess(lists.opening_guess()));
    }

    #[test]
    fn suggests_from_remaining() {
        let lists = shared_word_lists();
        let remaining = vec![w("crane"), w("grape")];
        let suggestion = suggest_guess(&lists, &remaining, &[]).unwrap();
        assert!(remaining.contains(&suggestion.word) || lists.is_valid_guess(suggestion.word));
    }

    #[test]
    fn opening_guess_is_instant() {
        let lists = shared_word_lists();
        let suggestion = suggest_guess(&lists, &lists.answers, &[]).unwrap();
        assert_eq!(suggestion.word, lists.opening_guess());
    }

    #[test]
    fn configurable_opening_is_used() {
        let lists = shared_word_lists();
        let opener = w("crane");
        let suggestion =
            suggest_guess_with_options(&lists, &lists.answers, &[], None, false, false, opener)
                .unwrap();
        assert_eq!(suggestion.word, opener);
    }

    #[test]
    fn suggestions_satisfy_hard_mode() {
        let lists = shared_word_lists();
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
        let lists = shared_word_lists();
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
        let lists = shared_word_lists();
        let history = vec![(w("slate"), pat("Gxxxx"))];
        let remaining = vec![w("crane")];
        assert!(compute_suggestion(&lists, &remaining, &history, Some(3), false).is_none());
    }

    #[test]
    fn single_remaining_compliant_returns_that_word() {
        let lists = shared_word_lists();
        let history = vec![(w("slate"), pat("xxxxx"))];
        let remaining = vec![w("crane")];
        let suggestion = compute_suggestion(&lists, &remaining, &history, Some(3), false).unwrap();
        assert_eq!(suggestion.word, w("crane"));
    }

    #[test]
    fn compute_suggestion_empty_pool_returns_none() {
        let lists = shared_word_lists();
        let history = vec![(w("aaaaa"), pat("GGGGG")), (w("bbbbb"), pat("GGGGG"))];
        let remaining = vec![w("crane")];
        assert!(compute_suggestion(&lists, &remaining, &history, None, false).is_none());
    }

    #[test]
    fn auto_solves_pound_and_wound() {
        let lists = shared_word_lists();
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
        let lists = shared_word_lists();
        let endgame_max = solver_config().endgame_probe_max_remaining;
        let mid_max = solver_config().minimax_midgame_max_remaining;
        // Use the full *ing cluster so remaining stays above endgame_probe_max_remaining
        // (widened beyond the legacy 16) while still exercising the offlist suffix probe.
        let remaining: Vec<Word> = ing_suffix_cluster();
        assert!(
            remaining.len() > endgame_max,
            "must skip endgame_pick to hit suffix off-list block (len={} endgame_max={})",
            remaining.len(),
            endgame_max
        );
        assert!(
            remaining.len() <= mid_max,
            "fixture should stay within mid-game upper bound if minimax ever runs"
        );
        let ctx = SolverContext::new(&lists, &remaining, &[], Some(1), false);
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
        let lists = shared_word_lists();
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

        let max_with = max_bucket_size(&lists, with_turns.word, &remaining);
        let max_open = max_bucket_size(&lists, open_ended.word, &remaining);
        // Turns-aware path must partition at least as well as open-ended and better than
        // naively guessing a remaining *ound answer (max_bucket 7).
        assert!(
            max_with <= 4,
            "turns-aware endgame pick {} max_bucket={max_with} (want <=4, not remaining-answer trap)",
            with_turns.word
        );
        assert!(
            max_with <= max_open,
            "turns-aware {} (max={max_with}) should not be worse than open-ended {} (max={max_open})",
            with_turns.word,
            open_ended.word
        );
        assert!(
            !remaining.contains(&with_turns.word),
            "with 8 remaining and 3 turns, should probe offlist, got {}",
            with_turns.word
        );
    }

    #[test]
    fn exact_endgame_solves_ound_cluster() {
        let lists = shared_word_lists();
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
        let suggestion = compute_suggestion(&lists, &remaining, &[], Some(3), false).unwrap();
        let max_b = max_bucket_size(&lists, suggestion.word, &remaining);
        let bound_max = max_bucket_size(&lists, w("bound"), &remaining);
        assert!(
            max_b <= 4,
            "endgame pick {} worst bucket {max_b} (bound trap is {bound_max})",
            suggestion.word
        );
        assert!(
            max_b < bound_max,
            "must beat guessing a remaining answer (bound max={bound_max})"
        );
        assert!(
            !remaining.contains(&suggestion.word),
            "should use offlist partition probe, got {}",
            suggestion.word
        );
    }

    #[test]
    fn suggestion_job_lifecycle_and_stale_generation() {
        let lists = shared_word_lists();
        let remaining = vec![w("crane"), w("grape"), w("trace")];
        let history = vec![];
        let job = spawn_suggestion_job(
            Arc::clone(&lists),
            remaining,
            history,
            5,
            false,
            OPENING_GUESS,
            1,
        );
        assert_eq!(job.generation(), 1);

        // Poll until ready (should be fast for 3 remaining).
        let mut result = None;
        for _ in 0..200 {
            if let Some(r) = job.try_recv() {
                result = r;
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(result.is_some(), "job should complete");

        // Stale generation: create job gen=2, but we only care try_recv discards mismatch —
        // spawn with gen 5 and drop without reading is fine; test discard path via channel.
        let (tx, rx) = mpsc::channel();
        tx.send((
            9u64,
            Some(Suggestion {
                word: w("crane"),
                entropy: 0.0,
                expected_remaining: 1.0,
            }),
        ))
        .unwrap();
        let stale_job = SuggestionJob { generation: 3, rx };
        // Generation mismatch → treated as no suggestion.
        match stale_job.try_recv() {
            Some(None) => {}
            other => panic!("expected Some(None) for stale generation, got {other:?}"),
        }
    }

    #[test]
    fn easy_mode_can_suggest_non_hard_mode_word() {
        let lists = shared_word_lists();
        // After green S at pos 0, hard mode requires S____; easy mode need not.
        let history = vec![(w("slate"), pat("Gxxxx"))];
        let remaining = filter_by_history(&lists.answers, &history);
        let hard = suggest_guess_with_options(
            &lists,
            &remaining,
            &history,
            Some(4),
            false,
            false,
            OPENING_GUESS,
        );
        if let Some(s) = hard {
            assert!(satisfies_hard_mode(s.word, &history));
        }
        let easy = suggest_guess_with_options(
            &lists,
            &remaining,
            &history,
            Some(4),
            false,
            true,
            OPENING_GUESS,
        );
        assert!(easy.is_some());
    }
}
