use std::sync::Arc;

use crate::core::filter::{filter_by_history, filter_candidates_in_place};
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
    history: Vec<(Word, Pattern)>,
    pub word_lists: Arc<WordLists>,
    remaining_answers: Vec<Word>,
}

impl GameState {
    pub fn new(word_lists: Arc<WordLists>) -> Self {
        let remaining_answers = word_lists.answers.clone();
        Self {
            turns: Vec::new(),
            history: Vec::new(),
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

    pub fn history(&self) -> &[(Word, Pattern)] {
        &self.history
    }

    pub fn suggest_next(&self) -> Option<Suggestion> {
        if self.is_solved() || self.is_lost() {
            return None;
        }

        let turns_left = 6usize.saturating_sub(self.turns.len());
        let suggestion = crate::core::solver::suggest_guess_interactive(
            &self.word_lists,
            &self.remaining_answers,
            &self.history,
            turns_left,
        )?;
        if !self.word_lists.is_valid_guess(suggestion.word) {
            return None;
        }
        Some(suggestion)
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

        if !satisfies_hard_mode(guess, &self.history) {
            return Err(GameError::HardModeViolation);
        }

        self.turns.push(Turn { guess, pattern });
        self.history.push((guess, pattern));
        filter_candidates_in_place(&mut self.remaining_answers, guess, pattern);
        Ok(())
    }

    pub fn undo_turn(&mut self) -> bool {
        if self.turns.pop().is_some() {
            self.history.pop();
            self.recompute_remaining();
            true
        } else {
            false
        }
    }

    pub fn reset(&mut self) {
        self.turns.clear();
        self.history.clear();
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
        self.remaining_answers =
            filter_by_history(&self.word_lists.answers, &self.history);
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
        Word::parse(s).unwrap()
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

    #[test]
    fn incremental_filter_matches_full_recompute() {
        let mut game = game();
        game.record_turn(w("slate"), pat("xxGGG")).unwrap();
        game.record_turn(w("crate"), pat("GGGGG")).unwrap();
        let incremental = game.remaining_answers().to_vec();
        game.reset();
        game.record_turn(w("slate"), pat("xxGGG")).unwrap();
        game.record_turn(w("crate"), pat("GGGGG")).unwrap();
        assert_eq!(incremental, game.remaining_answers());
    }

    #[test]
    fn interactive_suggestion_within_budget() {
        use std::time::Instant;

        use crate::core::solver::INTERACTIVE_SUGGESTION_BUDGET;

        let lists = Arc::new(crate::core::words::WordLists::load());
        let mut game = GameState::new(lists);
        game.record_turn(w("slate"), pat("xxxxx")).unwrap();

        let start = Instant::now();
        assert!(game.suggest_next().is_some());
        assert!(
            start.elapsed() <= INTERACTIVE_SUGGESTION_BUDGET,
            "suggest_next took {:.2}s (budget {:.0}s)",
            start.elapsed().as_secs_f64(),
            INTERACTIVE_SUGGESTION_BUDGET.as_secs_f64()
        );
    }
}
