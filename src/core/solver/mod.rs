mod auto_solve;
mod candidates;
mod context;
mod exact;
mod heuristics;
mod job;
mod orchestrate;
mod pool;
pub mod score;
mod second_guess;

use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::{WordLists, OPENING_GUESS};

pub use auto_solve::{auto_solve, auto_solve_with_options};
pub use job::{spawn_suggestion_job, SuggestionJob};
pub use score::{
    compare_final, compare_one_ply, score_one_ply, score_two_ply, GuessScore, PATTERN_BUCKETS,
};
pub use second_guess::lookup_second_guess;

pub(crate) use context::SolverContext;

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub word: Word,
    /// Information score in bits. Main path uses 2-ply entropy; early-return heuristics
    /// (endgame, minimax) use 1-ply entropy from `score_one_ply`. Opening uses a
    /// placeholder (`0.0`) because the opener is fixed with no startup computation.
    pub entropy: f64,
    pub expected_remaining: f64,
}

/// Whether to consult the precomputed second-guess table after the opener.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecondGuessMode {
    /// Use `data/second_guess_table.rs` when history is a single opener turn.
    UseTable,
    /// Always run live search (used when regenerating the table).
    Live,
}

/// Canonical suggestion request. Prefer this over the thin wrapper helpers.
#[derive(Clone, Copy)]
pub struct SuggestionRequest<'a> {
    pub word_lists: &'a WordLists,
    pub remaining: &'a [Word],
    pub history: &'a [(Word, Pattern)],
    pub turns_left: Option<usize>,
    pub interactive: bool,
    pub easy_mode: bool,
    pub opening: Word,
    pub second_guess: SecondGuessMode,
}

impl<'a> SuggestionRequest<'a> {
    pub fn new(
        word_lists: &'a WordLists,
        remaining: &'a [Word],
        history: &'a [(Word, Pattern)],
    ) -> Self {
        Self {
            word_lists,
            remaining,
            history,
            turns_left: None,
            interactive: false,
            easy_mode: false,
            opening: OPENING_GUESS,
            second_guess: SecondGuessMode::UseTable,
        }
    }

    pub fn turns_left(mut self, turns_left: Option<usize>) -> Self {
        self.turns_left = turns_left;
        self
    }

    pub fn interactive(mut self, interactive: bool) -> Self {
        self.interactive = interactive;
        self
    }

    pub fn easy_mode(mut self, easy_mode: bool) -> Self {
        self.easy_mode = easy_mode;
        self
    }

    pub fn opening(mut self, opening: Word) -> Self {
        self.opening = opening;
        self
    }

    pub fn second_guess(mut self, mode: SecondGuessMode) -> Self {
        self.second_guess = mode;
        self
    }
}

/// Max time for a single UI suggestion (after the user commits a turn).
pub fn interactive_suggestion_budget() -> std::time::Duration {
    crate::core::config::solver_config().interactive_budget()
}

/// Back-compat alias used by older call sites / tests.
pub const INTERACTIVE_SUGGESTION_BUDGET: std::time::Duration = std::time::Duration::from_secs(10);

/// Primary suggestion entry point.
pub fn suggest(req: SuggestionRequest<'_>) -> Option<Suggestion> {
    if req.remaining.is_empty() {
        return None;
    }

    if req.history.is_empty() && req.remaining.len() == req.word_lists.answers.len() {
        return Some(req.word_lists.opening_suggestion(req.opening));
    }

    orchestrate::compute_suggestion(req)
}

pub fn suggest_guess(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
) -> Option<Suggestion> {
    suggest(SuggestionRequest::new(
        word_lists,
        remaining_answers,
        history,
    ))
}

pub fn suggest_guess_with_turns(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
) -> Option<Suggestion> {
    suggest(SuggestionRequest::new(word_lists, remaining_answers, history).turns_left(turns_left))
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
    suggest(
        SuggestionRequest::new(word_lists, remaining_answers, history)
            .turns_left(turns_left)
            .interactive(interactive)
            .easy_mode(easy_mode)
            .opening(opening),
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

pub fn compute_suggestion(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    interactive: bool,
) -> Option<Suggestion> {
    orchestrate::compute_suggestion(
        SuggestionRequest::new(word_lists, remaining_answers, history)
            .turns_left(turns_left)
            .interactive(interactive),
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
    orchestrate::compute_suggestion(
        SuggestionRequest::new(word_lists, remaining_answers, history)
            .turns_left(turns_left)
            .interactive(interactive)
            .easy_mode(easy_mode),
    )
}

/// Like the normal path but never consults the second-guess table
/// (used when regenerating that table offline).
pub fn compute_suggestion_live(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    turns_left: Option<usize>,
    easy_mode: bool,
) -> Option<Suggestion> {
    orchestrate::compute_suggestion(
        SuggestionRequest::new(word_lists, remaining_answers, history)
            .turns_left(turns_left)
            .easy_mode(easy_mode)
            .second_guess(SecondGuessMode::Live),
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
    orchestrate::compute_suggestion(
        SuggestionRequest::new(word_lists, remaining_answers, history)
            .turns_left(turns_left)
            .interactive(interactive)
            .easy_mode(easy_mode)
            .opening(opening),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::thread;

    use super::candidates::shares_fixed_suffix;
    use super::exact::max_bucket_size;
    use super::pool::best_offlist_partition_probe;
    use super::*;
    use crate::core::config::solver_config;
    use crate::core::filter::filter_by_history;
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
            let remaining = filter_by_history(&lists.answers, history);
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
        let stale_job = SuggestionJob::from_parts(3, rx);
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

    #[test]
    fn suggestion_request_builder_matches_options_path() {
        let lists = shared_word_lists();
        let remaining = vec![w("crane"), w("grape")];
        let via_options = suggest_guess_with_options(
            &lists,
            &remaining,
            &[],
            Some(4),
            false,
            false,
            OPENING_GUESS,
        )
        .unwrap();
        let via_request = suggest(
            SuggestionRequest::new(&lists, &remaining, &[])
                .turns_left(Some(4))
                .opening(OPENING_GUESS),
        )
        .unwrap();
        assert_eq!(via_options.word, via_request.word);
    }
}
