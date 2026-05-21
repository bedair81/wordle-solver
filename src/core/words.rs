use std::collections::HashSet;
use std::sync::Arc;

use crate::core::word::Word;

const ANSWERS_RAW: &str = include_str!("../../data/answers.txt");
const ALLOWED_GUESSES_RAW: &str = include_str!("../../data/allowed_guesses.txt");

#[derive(Clone)]
pub struct WordLists {
    pub answers: Vec<Word>,
    pub guess_pool: Vec<Word>,
    answer_set: HashSet<Word>,
    guess_set: HashSet<Word>,
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

        Self {
            answers,
            guess_pool,
            answer_set,
            guess_set,
        }
    }

    pub fn is_valid_guess(&self, word: Word) -> bool {
        self.guess_set.contains(&word)
    }

    pub fn is_answer(&self, word: Word) -> bool {
        self.answer_set.contains(&word)
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
        assert!(lists.is_valid_guess(Word::from_str("slate").unwrap()));
        assert!(lists.is_answer(Word::from_str("crane").unwrap()));
    }
}
