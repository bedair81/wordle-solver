use crate::core::feedback::compute_feedback;
use crate::core::pattern::Pattern;
use crate::core::word::Word;

pub fn filter_candidates(candidates: &[Word], guess: Word, pattern: Pattern) -> Vec<Word> {
    candidates
        .iter()
        .copied()
        .filter(|&candidate| compute_feedback(guess, candidate) == pattern)
        .collect()
}

pub fn filter_by_history(candidates: &[Word], history: &[(Word, Pattern)]) -> Vec<Word> {
    let mut remaining = candidates.to_vec();
    for &(guess, pattern) in history {
        remaining = filter_candidates(&remaining, guess, pattern);
    }
    remaining
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pattern::Pattern;
    use crate::core::words::WordLists;

    fn w(s: &str) -> Word {
        Word::from_str(s).unwrap()
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
}
