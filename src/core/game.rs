use std::sync::Arc;

use crate::core::filter::filter_by_history;
use crate::core::hard_mode::satisfies_hard_mode;
use crate::core::pattern::Pattern;
use crate::core::solver::Suggestion;
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
    remaining_answers: Vec<Word>,
}

impl GameState {
    pub fn new(word_lists: Arc<WordLists>) -> Self {
        let remaining_answers = word_lists.answers.clone();
        Self {
            turns: Vec::new(),
            word_lists,
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

        let history: Vec<(Word, Pattern)> =
            self.turns.iter().map(|t| (t.guess, t.pattern)).collect();

        let turns_left = 6usize.saturating_sub(self.turns.len());
        crate::core::solver::suggest_guess_with_turns(
            &self.word_lists,
            &self.remaining_answers,
            &history,
            Some(turns_left),
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

        let history: Vec<(Word, Pattern)> =
            self.turns.iter().map(|t| (t.guess, t.pattern)).collect();
        if !satisfies_hard_mode(guess, &history) {
            return Err(GameError::HardModeViolation);
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

    pub fn is_solved(&self) -> bool {
        self.turns
            .last()
            .map(|t| t.pattern.is_win())
            .unwrap_or(false)
    }

    pub fn is_lost(&self) -> bool {
        self.turns.len() >= 6 && !self.is_solved()
    }

    fn recompute_remaining(&mut self) {
        let history: Vec<(Word, Pattern)> =
            self.turns.iter().map(|t| (t.guess, t.pattern)).collect();
        self.remaining_answers = filter_by_history(&self.word_lists.answers, &history);
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
                "NYT hard mode: keep green letters in place and include all yellow letters from prior guesses"
            ),
            GameError::GameOver => write!(f, "game is already over"),
        }
    }
}

impl std::error::Error for GameError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pattern::Pattern;
    use crate::core::word::Word;
    use std::sync::Arc;

    fn w(s: &str) -> Word {
        Word::from_str(s).unwrap()
    }

    fn pat(s: &str) -> Pattern {
        Pattern::from_str(s).unwrap()
    }

    fn game() -> GameState {
        GameState::new(Arc::new(crate::core::words::WordLists::load()))
    }

    #[test]
    fn record_turn_accepts_compliant_guess() {
        let mut game = game();
        let guess = w("slate");
        assert!(game.record_turn(guess, pat("Gxxxx")).is_ok());
    }

    #[test]
    fn record_turn_rejects_wrong_green_position() {
        let mut game = game();
        game.record_turn(w("slate"), pat("Gxxxx")).unwrap();
        assert_eq!(
            game.record_turn(w("plate"), pat("xxxxx")),
            Err(GameError::HardModeViolation)
        );
    }

    #[test]
    fn apply_turn_rejects_missing_yellow_letter() {
        let mut game = game();
        game.record_turn(w("crane"), pat("xxxYx")).unwrap();
        assert_eq!(
            game.apply_turn(w("slate"), pat("xxxxx")),
            Err(GameError::HardModeViolation)
        );
    }

    #[test]
    fn apply_turn_rejects_invalid_guess_word() {
        let mut game = game();
        let not_in_lists = w("qqqqq");
        assert_eq!(
            game.apply_turn(not_in_lists, pat("xxxxx")),
            Err(GameError::InvalidGuess(not_in_lists))
        );
    }

    #[test]
    fn record_turn_accepts_off_list_word() {
        let mut game = game();
        let off_list = w("qqqqq");
        assert!(game.record_turn(off_list, pat("xxxxx")).is_ok());
    }

    #[test]
    fn suggest_next_none_when_game_over() {
        let mut game = game();
        for _ in 0..6 {
            game.record_turn(w("slate"), pat("xxxxx")).unwrap();
        }
        assert!(game.is_lost());
        assert!(game.suggest_next().is_none());
    }

    #[test]
    fn commit_after_solved_returns_game_over() {
        let mut game = game();
        game.record_turn(w("slate"), pat("GGGGG")).unwrap();
        assert!(game.is_solved());
        assert_eq!(
            game.record_turn(w("crane"), pat("xxxxx")),
            Err(GameError::GameOver)
        );
    }
}
