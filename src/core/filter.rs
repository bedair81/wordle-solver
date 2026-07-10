use std::cell::RefCell;

use crate::core::feedback::compute_feedback;
use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

thread_local! {
    static FILTER_SCRATCH: RefCell<Vec<Word>> = const { RefCell::new(Vec::new()) };
}

/// Filter `candidates` in place without allocating a new vector.
pub fn filter_candidates_in_place(candidates: &mut Vec<Word>, guess: Word, pattern: Pattern) {
    candidates.retain(|&candidate| compute_feedback(guess, candidate) == pattern);
}

pub fn filter_candidates(candidates: &[Word], guess: Word, pattern: Pattern) -> Vec<Word> {
    candidates
        .iter()
        .copied()
        .filter(|&candidate| compute_feedback(guess, candidate) == pattern)
        .collect()
}

pub fn filter_by_history(candidates: &[Word], history: &[(Word, Pattern)]) -> Vec<Word> {
    if history.is_empty() {
        return candidates.to_vec();
    }

    FILTER_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.clear();
        scratch.extend_from_slice(candidates);
        for &(guess, pattern) in history {
            filter_candidates_in_place(&mut scratch, guess, pattern);
        }
        std::mem::take(&mut *scratch)
    })
}

/// Words in the guess pool that satisfy the full turn history but are not in the
/// bundled NYT answer list. Useful when feedback is consistent but `answers.txt`
/// is missing a word NYT accepted as a solution.
pub fn guess_pool_only_matches(word_lists: &WordLists, history: &[(Word, Pattern)]) -> Vec<Word> {
    if !filter_by_history(&word_lists.answers, history).is_empty() {
        return Vec::new();
    }
    filter_by_history(&word_lists.guess_pool, history)
        .into_iter()
        .filter(|&word| !word_lists.is_answer(word))
        .collect()
}

/// Why the answer list is empty after applying history (shared by TUI warning + render).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmptyCandidates {
    /// Feedback is consistent with guess-pool words missing from answers.txt.
    GuessPoolOnly { matches: Vec<Word> },
    /// This turn's feedback matches no bundled answer.
    FeedbackMatchesNoAnswer { guess: Word },
    /// Feedback contradicts earlier turns.
    ContradictoryHistory,
}

impl EmptyCandidates {
    pub fn classify(
        game: &crate::core::game::GameState,
        guess: Word,
        pattern: Pattern,
    ) -> Option<Self> {
        if game.is_solved() || game.remaining_count() > 0 {
            return None;
        }

        let pool_only = guess_pool_only_matches(&game.word_lists, game.history().as_slice());
        if !pool_only.is_empty() {
            return Some(Self::GuessPoolOnly { matches: pool_only });
        }

        let matches_any_answer = game
            .word_lists
            .answers
            .iter()
            .any(|&answer| compute_feedback(guess, answer) == pattern);

        if !matches_any_answer {
            Some(Self::FeedbackMatchesNoAnswer { guess })
        } else {
            Some(Self::ContradictoryHistory)
        }
    }

    pub fn warning_message(&self) -> String {
        match self {
            Self::GuessPoolOnly { matches } => {
                let sample: Vec<_> = matches.iter().take(5).map(|w| w.to_string()).collect();
                let extra = if matches.len() > sample.len() {
                    format!(" (+{} more)", matches.len() - sample.len())
                } else {
                    String::new()
                };
                format!(
                    "No candidates in our answer list, but these guess-pool words match your history: \
                     {}{}. NYT may use a word missing from answers.txt — try one of these, or run \
                     scripts/update-wordlists.sh.",
                    sample.join(", "),
                    extra
                )
            }
            Self::FeedbackMatchesNoAnswer { guess } => format!(
                "No candidates — this feedback matches no word in our answer list ({guess}). \
                 Check tile colors, or run scripts/update-wordlists.sh if today's NYT answer is missing."
            ),
            Self::ContradictoryHistory => {
                "No candidates — this feedback contradicts earlier turns. \
                 Double-check tile colors (duplicate letters are easy to mis-enter)."
                    .into()
            }
        }
    }

    pub fn short_message(&self) -> String {
        match self {
            Self::GuessPoolOnly { matches } => {
                let words: Vec<_> = matches.iter().take(8).map(|w| w.to_string()).collect();
                format!(
                    "No NYT answer-list matches. Guess-pool matches: {}.",
                    words.join(", ")
                )
            }
            Self::FeedbackMatchesNoAnswer { .. } | Self::ContradictoryHistory => {
                "No candidates match these constraints — check feedback or update word lists."
                    .into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pattern::Pattern;
    use crate::core::words::WordLists;

    fn w(s: &str) -> Word {
        Word::parse(s).unwrap()
    }

    fn pat(s: &str) -> Pattern {
        Pattern::from_str(s).unwrap()
    }

    #[test]
    fn filters_by_single_turn() {
        let lists = WordLists::load();
        let remaining = filter_candidates(&lists.answers, w("slate"), pat("xxGGG"));
        assert!(remaining.contains(&w("crate")));
        assert!(!remaining.contains(&w("crane")));
    }

    #[test]
    fn filters_by_history() {
        let lists = WordLists::load();
        let history = vec![(w("slate"), pat("xxGGG"))];
        let remaining = filter_by_history(&lists.answers, &history);
        assert!(remaining.iter().all(|w| {
            let s = w.as_str();
            s.as_bytes()[2] == b'a' && s.as_bytes()[3] == b't' && s.as_bytes()[4] == b'e'
        }));
    }

    fn filter_via_collect_chain(candidates: &[Word], history: &[(Word, Pattern)]) -> Vec<Word> {
        let mut remaining = candidates.to_vec();
        for &(guess, pattern) in history {
            remaining = filter_candidates(&remaining, guess, pattern);
        }
        remaining
    }

    #[test]
    fn filter_in_place_matches_collect() {
        let lists = WordLists::load();
        let history = vec![(w("slate"), pat("xxGGG")), (w("crate"), pat("xGxxx"))];
        let expected = filter_via_collect_chain(&lists.answers, &history);
        let mut in_place = lists.answers.clone();
        for &(guess, pattern) in &history {
            filter_candidates_in_place(&mut in_place, guess, pattern);
        }
        assert_eq!(in_place, expected);
    }

    #[test]
    fn filter_by_history_matches_collect_chain() {
        let lists = WordLists::load();
        let history = vec![(w("slate"), pat("xxGGG")), (w("crate"), pat("xGxxx"))];
        let via_history = filter_by_history(&lists.answers, &history);
        let via_collect = filter_via_collect_chain(&lists.answers, &history);
        assert_eq!(via_history, via_collect);
    }

    #[test]
    fn agree_remains_with_nyt_feedback() {
        use crate::core::feedback::compute_feedback;

        let lists = WordLists::load();
        let answer = w("agree");
        let history = vec![
            (w("audio"), compute_feedback(w("audio"), answer)),
            (w("alter"), compute_feedback(w("alter"), answer)),
            (w("apnea"), compute_feedback(w("apnea"), answer)),
        ];
        assert_eq!(history[1].1, pat("GxxGY")); // R is yellow for agree
        let remaining = filter_by_history(&lists.answers, &history);
        assert!(
            remaining.contains(&answer),
            "agree should remain; got {:?}",
            remaining.iter().take(10).collect::<Vec<_>>()
        );
    }

    #[test]
    fn filters_with_guess_outside_dictionary() {
        use crate::core::feedback::compute_feedback;

        let lists = WordLists::load();
        let answer = w("agree");
        let off_list = w("qqqqq");
        assert!(!lists.is_valid_guess(off_list));
        let history = vec![(off_list, compute_feedback(off_list, answer))];
        let remaining = filter_by_history(&lists.answers, &history);
        assert!(remaining.contains(&answer));
    }

    #[test]
    fn emoji_matches_history_in_answer_list() {
        let lists = WordLists::load();
        let history = vec![
            (w("slate"), pat("xxxxY")),
            (w("diner"), pat("xYxYx")),
            (w("weigh"), pat("xYYxx")),
            (w("equip"), pat("GxxYx")),
        ];
        assert!(lists.is_answer(w("emoji")));
        let remaining = filter_by_history(&lists.answers, &history);
        assert_eq!(remaining, vec![w("emoji")]);
        assert!(guess_pool_only_matches(&lists, &history).is_empty());
    }
}
