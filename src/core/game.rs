use std::sync::Arc;

use crate::core::filter::filter_by_history;
use crate::core::pattern::Pattern;
use crate::core::hard_mode::satisfies_hard_mode;
use crate::core::solver::{suggest_guess, Suggestion};
use crate::core::word::Word;
use crate::core::words::WordLists;

#[derive(Clone, Debug)]
pub struct Turn {
    pub guess: Word,
    pub pattern: Pattern,
}

#[derive(Clone)]
pub struct GameState {
    pub turns: Vec<Turn>,
    pub word_lists: Arc<WordLists>,
    pub hard_mode: bool,
    remaining_answers: Vec<Word>,
}

impl GameState {
    pub fn new(word_lists: Arc<WordLists>, hard_mode: bool) -> Self {
        let remaining_answers = word_lists.answers.clone();
        Self {
            turns: Vec::new(),
            word_lists,
            hard_mode,
            remaining_answers,
        }
    }

    pub fn remaining_answers(&self) -> &[Word] {
        &self.remaining_answers
    }

    pub fn remaining_count(&self) -> usize {
        self.remaining_answers.len()
    }

    pub fn suggest_next(&self) -> Option<Suggestion> {
        if self.is_solved() || self.is_lost() {
            return None;
        }

        let history: Vec<(Word, Pattern)> = self
            .turns
            .iter()
            .map(|t| (t.guess, t.pattern))
            .collect();

        suggest_guess(
            &self.word_lists,
            &self.remaining_answers,
            &history,
            self.hard_mode,
        )
    }

    pub fn apply_turn(&mut self, guess: Word, pattern: Pattern) -> Result<(), GameError> {
        self.commit_turn(guess, pattern, true)
    }

    /// Record a turn from Solver Aid — any 5-letter guess is allowed (NYT may accept
    /// words outside our cached guess list).
    pub fn record_turn(&mut self, guess: Word, pattern: Pattern) -> Result<(), GameError> {
        self.commit_turn(guess, pattern, false)
    }

    fn commit_turn(
        &mut self,
        guess: Word,
        pattern: Pattern,
        require_dictionary_guess: bool,
    ) -> Result<(), GameError> {
        if self.is_solved() || self.is_lost() {
            return Err(GameError::GameOver);
        }

        if require_dictionary_guess && !self.word_lists.is_valid_guess(guess) {
            return Err(GameError::InvalidGuess(guess));
        }

        if self.hard_mode {
            let history: Vec<(Word, Pattern)> = self
                .turns
                .iter()
                .map(|t| (t.guess, t.pattern))
                .collect();
            if !satisfies_hard_mode(guess, &history) {
                return Err(GameError::HardModeViolation);
            }
        }

        self.turns.push(Turn { guess, pattern });
        self.recompute_remaining();
        Ok(())
    }

    pub fn undo_turn(&mut self) -> bool {
        if self.turns.pop().is_some() {
            self.recompute_remaining();
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        self.turns.clear();
        self.remaining_answers = self.word_lists.answers.clone();
    }

    pub fn toggle_hard_mode(&mut self) {
        self.hard_mode = !self.hard_mode;
    }

    pub fn is_solved(&self) -> bool {
        self.turns.last().map(|t| t.pattern.is_win()).unwrap_or(false)
    }

    pub fn is_lost(&self) -> bool {
        self.turns.len() >= 6 && !self.is_solved()
    }

    fn recompute_remaining(&mut self) {
        let history: Vec<(Word, Pattern)> = self
            .turns
            .iter()
            .map(|t| (t.guess, t.pattern))
            .collect();
        self.remaining_answers =
            filter_by_history(&self.word_lists.answers, &history);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameError {
    InvalidGuess(Word),
    HardModeViolation,
    GameOver,
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameError::InvalidGuess(w) => write!(f, "'{w}' is not a valid Wordle guess"),
            GameError::HardModeViolation => write!(
                f,
                "hard mode: use all green letters in place and include all yellow letters from prior guesses"
            ),
            GameError::GameOver => write!(f, "game is already over"),
        }
    }
}

impl std::error::Error for GameError {}
