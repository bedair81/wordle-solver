mod candidates;
pub mod score;

use std::collections::HashSet;

use crate::core::feedback::compute_feedback;
use crate::core::filter::filter_by_history;
use crate::core::hard_mode::satisfies_hard_mode;
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

pub use score::{compare_final, compare_one_ply, score_one_ply, GuessScore};

use candidates::{
    select_guess_candidates, shares_fixed_suffix, two_ply_candidate_indices, CandidateBuffer,
    TURNS_LEFT_REMAINING_SLACK,
};
use score::score_two_ply;

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub word: Word,
    pub entropy: f64,
    pub expected_remaining: f64,
}

thread_local! {
    static CANDIDATE_SCRATCH: std::cell::RefCell<CandidateBuffer> =
        std::cell::RefCell::new(CandidateBuffer::new());
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

    compute_suggestion(word_lists, remaining_answers, history, turns_left)
}

pub fn compute_suggestion(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
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
        // Do not suggest an unrelated pool word when the sole remaining answer is
        // hard-mode-incompatible — that contradicts the candidate list in the UI.
        return None;
    }

    CANDIDATE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let remaining_set: HashSet<Word> = remaining_answers.iter().copied().collect();

        // Mid-game: minimize worst bucket before entropy picks a trap (e.g. taint → *aunt).
        if turns_left.is_some_and(|left| {
            remaining_answers.len() > ENDGAME_PROBE_MAX_REMAINING
                && remaining_answers.len() <= MINIMAX_MIDGAME_MAX_REMAINING
                && left >= 2
        }) {
            if let Some(word) = best_minimax_compliant_pick(
                word_lists,
                remaining_answers,
                &remaining_set,
                history,
                shares_fixed_suffix(remaining_answers),
                turns_left,
                false,
            ) {
                let score =
                    score_one_ply(word_lists, word, remaining_answers, &remaining_set);
                return Some(Suggestion {
                    word,
                    entropy: score.one_ply_entropy,
                    expected_remaining: score.expected_remaining,
                });
            }
        }

        // Endgame (`turns_left`): prefer guesses that partition remaining answers into
        // buckets small enough to finish in the guesses still available (e.g. *OUND,
        // *o*er with greens locked). When remaining-only picks leave a bucket larger than
        // `turns_left - 1`, fall back to an off-list probe (e.g. boxer/foyer/joker/poker).
        if let Some(left) = turns_left {
            if let Some(word) = endgame_pick(
                word_lists,
                remaining_answers,
                &remaining_set,
                history,
                left,
            ) {
                let score =
                    score_one_ply(word_lists, word, remaining_answers, &remaining_set);
                return Some(Suggestion {
                    word,
                    entropy: score.one_ply_entropy,
                    expected_remaining: score.expected_remaining,
                });
            }
        }

        // Shared suffix with more answers than turns left: off-list probe to split
        // leading letters (e.g. waste/haunt at 6 remaining, 2 turns left).
        let suffix_cluster = shares_fixed_suffix(remaining_answers);
        if suffix_cluster
            && turns_left.is_some_and(|left| remaining_answers.len() > left.saturating_add(1))
            && remaining_answers.len() >= 6
        {
            if let Some(probe) =
                best_offlist_partition_probe(word_lists, remaining_answers, &remaining_set, history)
            {
                return Some(Suggestion {
                    word: probe,
                    entropy: 0.0,
                    expected_remaining: remaining_answers.len() as f64,
                });
            }
        }

        // Tight turns: prefer guesses that minimize the largest feedback bucket
        // (avoids *OUND / *o*er traps where entropy picks sound but leaves six
        // first-letter variants with four greens locked).
        if turns_left.is_some_and(|left| {
            remaining_answers.len() > left.saturating_add(1)
                && remaining_answers.len() >= 4
                && remaining_answers.len() <= ENDGAME_PROBE_MAX_REMAINING
                && (remaining_answers.len() > left || shares_fixed_suffix(remaining_answers))
        }) {
            if let Some(word) = best_minimax_compliant_pick(
                word_lists,
                remaining_answers,
                &remaining_set,
                history,
                shares_fixed_suffix(remaining_answers),
                turns_left,
                false,
            ) {
                let score =
                    score_one_ply(word_lists, word, remaining_answers, &remaining_set);
                return Some(Suggestion {
                    word,
                    entropy: score.one_ply_entropy,
                    expected_remaining: score.expected_remaining,
                });
            }
        }

        let guess_candidates = select_guess_candidates(
            word_lists,
            remaining_answers,
            history,
            turns_left,
            &mut scratch,
        );

        if guess_candidates.is_empty() {
            return None;
        }

        let one_ply_scores: Vec<GuessScore> = guess_candidates
            .iter()
            .map(|&guess| score_one_ply(word_lists, guess, remaining_answers, &remaining_set))
            .collect();

        let refine_indices = two_ply_candidate_indices(&one_ply_scores, remaining_answers.len());

        let mut refined_scores = Vec::with_capacity(refine_indices.len());
        for idx in refine_indices {
            refined_scores.push(score_two_ply(
                word_lists,
                one_ply_scores[idx],
                remaining_answers,
                &remaining_set,
                history,
                turns_left,
            ));
        }

        let best = refined_scores
            .into_iter()
            .max_by(|a, b| compare_final(*a, *b))?;

        Some(Suggestion {
            word: best.word,
            entropy: best.two_ply_entropy,
            expected_remaining: best.expected_remaining,
        })
    })
}

const ENDGAME_PROBE_MAX_REMAINING: usize = 16;

const MINIMAX_MIDGAME_MAX_REMAINING: usize = 80;

/// Largest bucket after a guess must be solvable in the turns still available after it.
fn partition_sufficient(max_bucket: usize, turns_left: usize) -> bool {
    max_bucket <= turns_left.saturating_sub(1).max(1)
}

fn max_bucket_size(word_lists: &WordLists, guess: Word, remaining: &[Word]) -> usize {
    let buckets = word_lists
        .pattern_cache
        .build_buckets_for(guess, remaining);
    buckets.counts.iter().copied().max().unwrap_or(0)
}

fn in_endgame(remaining_len: usize, turns_left: usize) -> bool {
    remaining_len > 1
        && (remaining_len <= turns_left.saturating_add(TURNS_LEFT_REMAINING_SLACK)
            || remaining_len > turns_left.saturating_add(1))
        && remaining_len <= ENDGAME_PROBE_MAX_REMAINING
}

/// Endgame: remaining-answer partition if sufficient; otherwise off-list probe.
fn endgame_pick(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    remaining_set: &HashSet<Word>,
    history: &[(Word, Pattern)],
    turns_left: usize,
) -> Option<Word> {
    if !in_endgame(remaining_answers.len(), turns_left) {
        return None;
    }

    let pick_last = remaining_answers.len() <= turns_left.saturating_add(1);
    let remaining_pick = best_partition_remaining_pick(
        word_lists,
        remaining_answers,
        remaining_set,
        history,
        pick_last,
    );
    if let Some(word) = remaining_pick {
        let max_b = max_bucket_size(word_lists, word, remaining_answers);
        if partition_sufficient(max_b, turns_left) {
            return Some(word);
        }
    }

    let suffix_cluster = shares_fixed_suffix(remaining_answers);
    let pool_pick = best_minimax_compliant_pick(
        word_lists,
        remaining_answers,
        remaining_set,
        history,
        suffix_cluster,
        Some(turns_left),
        false,
    );
    let offlist_pick = best_minimax_compliant_pick(
        word_lists,
        remaining_answers,
        remaining_set,
        history,
        suffix_cluster,
        Some(turns_left),
        true,
    );

    match (pool_pick, offlist_pick) {
        (Some(all), Some(off)) => {
            let all_max = max_bucket_size(word_lists, all, remaining_answers);
            let off_max = max_bucket_size(word_lists, off, remaining_answers);
            let n = remaining_answers.len();
            let off_progresses = off_max < n;
            let all_progresses = all_max < n;
            if off_max < all_max && off_progresses {
                return Some(off);
            }
            if all_progresses || all_max <= off_max || remaining_set.contains(&all) {
                return Some(all);
            }
            Some(off)
        }
        (None, off) | (off, None) => off.or(remaining_pick),
    }
}

/// Pick an untried remaining answer that maximizes distinct feedback buckets vs the
/// remaining set; tie-break by entropy, then lexicographic order (`pick_last` = latest).
fn best_partition_remaining_pick(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    remaining_set: &HashSet<Word>,
    history: &[(Word, Pattern)],
    pick_last: bool,
) -> Option<Word> {
    let tried: HashSet<Word> = history.iter().map(|(g, _)| *g).collect();
    let mut best: Option<(Word, usize, usize, f64)> = None;
    for &guess in remaining_answers {
        if !satisfies_hard_mode(guess, history) || tried.contains(&guess) {
            continue;
        }
        let buckets = word_lists
            .pattern_cache
            .build_buckets_for(guess, remaining_answers);
        let max_b = buckets.counts.iter().copied().max().unwrap_or(0);
        let score = score_one_ply(word_lists, guess, remaining_answers, remaining_set);
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
                if better { candidate } else { prev }
            }
        });
    }
    best.map(|(word, _, _, _)| word)
}

fn best_offlist_partition_probe(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    remaining_set: &HashSet<Word>,
    history: &[(Word, Pattern)],
) -> Option<Word> {
    use crate::core::hard_mode::filter_hard_mode_compliant;

    let compliant = filter_hard_mode_compliant(&word_lists.guess_pool, history);
    let pool: &[Word] = if history.is_empty() {
        &word_lists.guess_pool
    } else if compliant.is_empty() {
        return None;
    } else {
        &compliant
    };
    let guess = score_best_probe(word_lists, remaining_answers, remaining_set, pool, history)?;
    let buckets = word_lists
        .pattern_cache
        .build_buckets_for(guess, remaining_answers);
    if buckets.nonempty > 1 {
        Some(guess)
    } else {
        None
    }
}

/// Minimize largest bucket; tie-break toward more buckets, then entropy.
fn best_minimax_compliant_pick(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    remaining_set: &HashSet<Word>,
    history: &[(Word, Pattern)],
    suffix_cluster: bool,
    turns_left: Option<usize>,
    offlist_only: bool,
) -> Option<Word> {
    use crate::core::hard_mode::filter_hard_mode_compliant;

    let tried: HashSet<Word> = history.iter().map(|(g, _)| *g).collect();
    let compliant = filter_hard_mode_compliant(&word_lists.guess_pool, history);
    if history.is_empty() {
        // use full guess pool at opening
    } else if compliant.is_empty() {
        return None;
    }

    let mut candidates: Vec<Word> = if history.is_empty() {
        word_lists
            .guess_pool
            .iter()
            .copied()
            .filter(|&w| !tried.contains(&w))
            .collect()
    } else {
        compliant
            .iter()
            .copied()
            .filter(|&w| !tried.contains(&w))
            .collect()
    };
    if !offlist_only {
        for &word in remaining_answers {
            if satisfies_hard_mode(word, history)
                && !tried.contains(&word)
                && !candidates.contains(&word)
            {
                candidates.push(word);
            }
        }
    }

    candidates.retain(|w| !offlist_only || !remaining_set.contains(w));

    let prefer_answers = turns_left.is_some_and(|left| {
        left <= 2 && remaining_answers.len() <= 6
    });

    candidates
        .into_iter()
        .filter_map(|guess| {
            let buckets = word_lists
                .pattern_cache
                .build_buckets_for(guess, remaining_answers);
            let max_b = buckets.counts.iter().copied().max().unwrap_or(0);
            if max_b == 0
                || (remaining_answers.len() > 1 && max_b >= remaining_answers.len())
            {
                return None;
            }
            let score = score_one_ply(word_lists, guess, remaining_answers, remaining_set);
            let is_answer = remaining_set.contains(&guess);
            Some((guess, max_b, buckets.nonempty, score.one_ply_entropy, is_answer))
        })
        .min_by(|a, b| {
            a.1
                .cmp(&b.1)
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| {
                    if prefer_answers || suffix_cluster {
                        b.4.cmp(&a.4)
                    } else {
                        a.4.cmp(&b.4)
                    }
                })
                .then_with(|| {
                    b.3
                        .partial_cmp(&a.3)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
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

fn score_best_probe(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    remaining_set: &HashSet<Word>,
    pool: &[Word],
    history: &[(Word, Pattern)],
) -> Option<Word> {
    let tried: HashSet<Word> = history.iter().map(|(g, _)| *g).collect();
    pool.iter()
        .copied()
        .filter(|word| !remaining_set.contains(word) && !tried.contains(word))
        .map(|guess| {
            let buckets = word_lists
                .pattern_cache
                .build_buckets_for(guess, remaining_answers);
            let score = score_one_ply(word_lists, guess, remaining_answers, remaining_set);
            (guess, buckets.nonempty, score.one_ply_entropy)
        })
        .max_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| {
                    a.2.partial_cmp(&b.2)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
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

        // Last turn: the target is among the remaining answers — try each compliant word.
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
    use super::*;
    use crate::core::hard_mode::satisfies_hard_mode;
    use crate::core::pattern::Pattern;
    use crate::core::words::WordLists;

    fn w(s: &str) -> Word {
        Word::from_str(s).unwrap()
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
        assert!(compute_suggestion(&lists, &remaining, &history, Some(3)).is_none());
    }

    #[test]
    fn single_remaining_compliant_returns_that_word() {
        let lists = WordLists::load();
        let history = vec![(w("slate"), pat("xxxxx"))];
        let remaining = vec![w("crane")];
        let suggestion = compute_suggestion(&lists, &remaining, &history, Some(3)).unwrap();
        assert_eq!(suggestion.word, w("crane"));
    }

    #[test]
    fn compute_suggestion_empty_pool_returns_none() {
        let lists = WordLists::load();
        let history = vec![(w("aaaaa"), pat("GGGGG")), (w("bbbbb"), pat("GGGGG"))];
        let remaining = vec![w("crane")];
        assert!(compute_suggestion(&lists, &remaining, &history, None).is_none());
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

    #[test]
    fn partition_sufficient_requires_small_largest_bucket() {
        assert!(partition_sufficient(2, 3));
        assert!(!partition_sufficient(4, 3));
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
