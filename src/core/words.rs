use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use crate::core::patterns::PatternCache;
use crate::core::word::Word;
use crate::core::solver::Suggestion;

const ANSWERS_RAW: &str = include_str!("../../data/answers.txt");
const ALLOWED_GUESSES_RAW: &str = include_str!("../../data/allowed_guesses.txt");

/// Hardcoded strong opener — avoids a multi-minute opening computation at startup.
pub const OPENING_GUESS: Word = Word(*b"slate");

#[derive(Clone)]
pub struct WordLists {
    pub answers: Vec<Word>,
    pub guess_pool: Vec<Word>,
    pub pattern_cache: PatternCache,
    answer_set: HashSet<Word>,
    guess_set: HashSet<Word>,
    opening: Arc<OnceLock<Suggestion>>,
}

impl WordLists {
    pub fn load() -> Self {
        let answers = parse_words(ANSWERS_RAW);
        let extra_guesses = parse_words(ALLOWED_GUESSES_RAW);

        let answer_set: HashSet<Word> = answers.iter().copied().collect();
        let mut guess_set = answer_set.clone();
        let mut guess_pool = answers.clone();

        for word in extra_guesses {
            if guess_set.insert(word) {
                guess_pool.push(word);
            }
        }

        guess_pool.sort();
        let pattern_cache = PatternCache::build(&answers, &guess_pool);

        Self {
            answers,
            guess_pool,
            pattern_cache,
            answer_set,
            guess_set,
            opening: Arc::new(OnceLock::new()),
        }
    }

    pub fn is_valid_guess(&self, word: Word) -> bool {
        self.guess_set.contains(&word)
    }

    pub fn is_answer(&self, word: Word) -> bool {
        self.answer_set.contains(&word)
    }

    pub fn opening_suggestion(&self) -> Suggestion {
        self.opening
            .get_or_init(|| Suggestion {
                word: OPENING_GUESS,
                entropy: 0.0,
                expected_remaining: self.answers.len() as f64,
            })
            .clone()
    }

    pub fn opening_guess(&self) -> Word {
        OPENING_GUESS
    }
}

fn parse_words(raw: &str) -> Vec<Word> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            Word::from_str(line)
        })
        .collect()
}

pub fn default_word_lists() -> Arc<WordLists> {
    Arc::new(WordLists::load())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_expected_counts() {
        let lists = WordLists::load();
        assert!(lists.answers.len() >= 2300);
        assert!(lists.guess_pool.len() >= 12000);
    }

    #[test]
    fn pattern_cache_matches_feedback() {
        use crate::core::feedback::compute_feedback;
        use crate::core::solver::score::pattern_bucket_index;

        let lists = WordLists::load();
        let guess = Word::from_str("slate").unwrap();
        let answer = Word::from_str("crate").unwrap();
        let expected = pattern_bucket_index(compute_feedback(guess, answer));
        assert_eq!(lists.pattern_cache.bucket(guess, answer), Some(expected));
    }
}