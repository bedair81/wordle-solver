//! Pure play-state machine (no ratatui). Unit-testable without a terminal.

use std::sync::Arc;

use wordle_solver::core::feedback::compute_feedback;
use wordle_solver::core::filter::guess_pool_only_matches;
use wordle_solver::core::game::{GameError, GameState};
use wordle_solver::core::hard_mode::{
    assemble_guess, editable_slot_count, known_green_letters, prefill_feedback_tiles,
    satisfies_hard_mode,
};
use wordle_solver::core::pattern::{Pattern, Tile};
use wordle_solver::core::session::{save_session, SessionSnapshot};
use wordle_solver::core::solver::{spawn_suggestion_job, Suggestion, SuggestionJob};
use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

use crate::tui::input::Action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputPhase {
    TypingGuess,
    SettingFeedback,
}

pub struct PlayState {
    pub game: GameState,
    pub copilot: bool,
    pub phase: InputPhase,
    pub guess_buffer: String,
    pub feedback_tiles: [Option<Tile>; 5],
    pub feedback_cursor: usize,
    pub error: Option<String>,
    pub list_scroll: usize,
    pub show_help: bool,
    pub title: &'static str,
    pub colorblind: bool,
    pub thinking: bool,
    cached_suggestion: Option<Suggestion>,
    constraint_warning: Option<String>,
    fixed_letters: [Option<u8>; 5],
    pending_guess: Option<Word>,
    suggestion_generation: u64,
    suggestion_job: Option<SuggestionJob>,
    session_path: Option<std::path::PathBuf>,
}

impl PlayState {
    pub fn new(
        word_lists: Arc<WordLists>,
        copilot: bool,
        title: &'static str,
        easy_mode: bool,
        opening: Word,
        colorblind: bool,
        session_path: Option<std::path::PathBuf>,
    ) -> Self {
        let mut state = Self {
            game: GameState::with_options(word_lists, easy_mode, opening),
            copilot,
            phase: if copilot {
                InputPhase::SettingFeedback
            } else {
                InputPhase::TypingGuess
            },
            guess_buffer: String::new(),
            feedback_tiles: [None; 5],
            feedback_cursor: 0,
            error: None,
            list_scroll: 0,
            show_help: false,
            title,
            colorblind,
            thinking: false,
            cached_suggestion: None,
            constraint_warning: None,
            fixed_letters: [None; 5],
            pending_guess: None,
            suggestion_generation: 0,
            suggestion_job: None,
            session_path,
        };
        state.request_suggestion();
        if state.copilot {
            // Opening is instant; poll once so copilot can sync.
            state.poll_suggestion();
            state.sync_copilot_guess();
        } else {
            state.begin_typing_phase();
            state.poll_suggestion();
        }
        state
    }

    pub fn cached_suggestion(&self) -> Option<&Suggestion> {
        self.cached_suggestion.as_ref()
    }

    pub fn constraint_warning(&self) -> Option<&str> {
        self.constraint_warning.as_deref()
    }

    pub fn fixed_letters(&self) -> [Option<u8>; 5] {
        self.fixed_letters
    }

    fn invalidate_suggestion_job(&mut self) {
        self.suggestion_generation = self.suggestion_generation.wrapping_add(1);
        self.suggestion_job = None;
        self.thinking = false;
    }

    /// Start a background suggestion compute (non-blocking).
    pub fn request_suggestion(&mut self) {
        if self.game.is_solved() || self.game.is_lost() {
            self.cached_suggestion = None;
            self.thinking = false;
            self.suggestion_job = None;
            return;
        }

        // Opening path is instant — compute synchronously.
        if self.game.turns.is_empty() {
            self.cached_suggestion = self.game.suggest_next();
            self.thinking = false;
            self.suggestion_job = None;
            return;
        }

        self.suggestion_generation = self.suggestion_generation.wrapping_add(1);
        let gen = self.suggestion_generation;
        let turns_left = 6usize.saturating_sub(self.game.turns.len());
        let history = self.game.history();
        let remaining = self.game.remaining_answers().to_vec();
        let job = spawn_suggestion_job(
            Arc::clone(&self.game.word_lists),
            remaining,
            history,
            turns_left,
            self.game.easy_mode(),
            self.game.opening(),
            gen,
        );
        self.suggestion_job = Some(job);
        self.thinking = true;
        // Keep previous suggestion visible until the new one arrives.
    }

    /// Poll background job; apply result if ready and generation matches.
    pub fn poll_suggestion(&mut self) -> bool {
        let Some(job) = self.suggestion_job.as_ref() else {
            return false;
        };
        let Some(result) = job.try_recv() else {
            return false;
        };
        self.suggestion_job = None;
        self.thinking = false;
        self.cached_suggestion = result;
        true
    }

    fn sync_copilot_guess(&mut self) {
        let Some(word) = self.cached_suggestion.as_ref().map(|s| s.word) else {
            self.guess_buffer.clear();
            self.pending_guess = None;
            self.feedback_tiles = [None; 5];
            self.feedback_cursor = 0;
            if !self.thinking {
                self.error = Some(
                    "No NYT hard-mode-compliant guess available — check turn history or remaining candidates."
                        .into(),
                );
            }
            return;
        };
        self.guess_buffer = word.as_str().to_string();
        if !self.begin_feedback_phase(word) {
            self.error = Some(
                "Solver could not suggest a NYT hard-mode-compliant guess — check turn history."
                    .into(),
            );
        }
    }

    fn turn_history(&self) -> Vec<(Word, Pattern)> {
        self.game.history()
    }

    fn begin_typing_phase(&mut self) {
        self.fixed_letters = if self.game.easy_mode() {
            [None; 5]
        } else {
            known_green_letters(&self.turn_history())
        };
        self.guess_buffer.clear();
        self.pending_guess = None;
        self.feedback_tiles = [None; 5];
        self.feedback_cursor = 0;
        self.phase = InputPhase::TypingGuess;
    }

    /// After replaying turns from a saved session, refresh suggestion and UI phase.
    pub fn after_session_restore(&mut self) {
        self.error = None;
        self.constraint_warning = None;
        self.request_suggestion();
        for _ in 0..50 {
            if self.poll_suggestion() || !self.thinking {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if self.copilot {
            self.prepare_copilot_feedback();
        } else {
            self.begin_typing_phase();
        }
    }

    pub fn begin_feedback_phase(&mut self, guess: Word) -> bool {
        let history = self.turn_history();
        if !self.game.easy_mode() && !satisfies_hard_mode(guess, &history) {
            self.error = Some(GameError::HardModeViolation.to_string());
            return false;
        }
        self.error = None;
        self.pending_guess = Some(guess);
        let (tiles, cursor) = if self.game.easy_mode() {
            ([None; 5], 0)
        } else {
            prefill_feedback_tiles(&history, guess)
        };
        self.feedback_tiles = tiles;
        self.feedback_cursor = cursor;
        self.phase = InputPhase::SettingFeedback;
        true
    }

    fn is_feedback_locked(&self, index: usize) -> bool {
        !self.game.easy_mode() && self.feedback_tiles[index] == Some(Tile::Correct)
    }

    fn move_feedback_cursor(&mut self, delta: i32) {
        let mut i = self.feedback_cursor as i32;
        for _ in 0..5 {
            i = (i + delta).clamp(0, 4);
            if !self.is_feedback_locked(i as usize) {
                self.feedback_cursor = i as usize;
                return;
            }
        }
    }

    pub fn active_guess(&self) -> Option<Word> {
        if self.copilot {
            self.cached_suggestion.as_ref().map(|s| s.word)
        } else if let Some(guess) = self.pending_guess {
            Some(guess)
        } else {
            assemble_guess(&self.fixed_letters, &self.guess_buffer)
        }
    }

    fn editable_slots(&self) -> usize {
        editable_slot_count(&self.fixed_letters)
    }

    fn prepare_copilot_feedback(&mut self) {
        self.sync_copilot_guess();
    }

    fn persist_session(&self) {
        let Some(path) = &self.session_path else {
            return;
        };
        if self.game.turns.is_empty() || self.game.is_solved() || self.game.is_lost() {
            let _ = wordle_solver::core::session::clear_session(path);
            return;
        }
        let snap = SessionSnapshot::from_game(
            &self.game,
            self.copilot,
            self.colorblind,
            self.game.opening(),
        );
        let _ = save_session(path, &snap);
    }

    /// Handle an input action. Returns true if the user requested exit to menu.
    pub fn handle(&mut self, action: Action) -> bool {
        // Always allow quit/back while thinking.
        match action {
            Action::Quit | Action::Back => return true,
            Action::ToggleColorblind => {
                self.colorblind = !self.colorblind;
                return false;
            }
            _ => {}
        }

        match action {
            Action::Help => {
                self.show_help = !self.show_help;
            }
            Action::Undo => {
                if self.game.undo_turn() {
                    self.error = None;
                    self.constraint_warning = None;
                    self.invalidate_suggestion_job();
                    self.request_suggestion();
                    self.poll_suggestion();
                    self.persist_session();
                    if self.copilot {
                        self.prepare_copilot_feedback();
                    } else {
                        self.begin_typing_phase();
                    }
                }
            }
            Action::Reset => {
                self.game.reset();
                self.error = None;
                self.constraint_warning = None;
                self.list_scroll = 0;
                self.invalidate_suggestion_job();
                self.request_suggestion();
                self.poll_suggestion();
                self.persist_session();
                if self.copilot {
                    self.phase = InputPhase::SettingFeedback;
                    self.prepare_copilot_feedback();
                } else {
                    self.begin_typing_phase();
                }
            }
            Action::Up => {
                if self.list_scroll > 0 {
                    self.list_scroll -= 1;
                }
            }
            Action::Down => {
                let max = self.game.remaining_count().saturating_sub(1);
                if self.list_scroll < max {
                    self.list_scroll += 1;
                }
            }
            Action::Char(c) if self.phase == InputPhase::TypingGuess && !self.copilot => {
                if self.guess_buffer.len() < self.editable_slots() {
                    self.guess_buffer.push(c);
                    self.error = None;
                }
            }
            Action::Delete if self.phase == InputPhase::TypingGuess && !self.copilot => {
                self.guess_buffer.pop();
                self.error = None;
            }
            Action::Submit => {
                self.on_submit();
            }
            Action::SetTileCorrect if self.phase == InputPhase::SettingFeedback => {
                if !self.is_feedback_locked(self.feedback_cursor) {
                    self.feedback_tiles[self.feedback_cursor] = Some(Tile::Correct);
                }
            }
            Action::SetTilePresent if self.phase == InputPhase::SettingFeedback => {
                if !self.is_feedback_locked(self.feedback_cursor) {
                    self.feedback_tiles[self.feedback_cursor] = Some(Tile::Present);
                }
            }
            Action::SetTileAbsent if self.phase == InputPhase::SettingFeedback => {
                if !self.is_feedback_locked(self.feedback_cursor) {
                    self.feedback_tiles[self.feedback_cursor] = Some(Tile::Absent);
                }
            }
            Action::CycleTile if self.phase == InputPhase::SettingFeedback => {
                if !self.is_feedback_locked(self.feedback_cursor) {
                    let cur = self.feedback_tiles[self.feedback_cursor]
                        .unwrap_or(Tile::Absent)
                        .cycle();
                    self.feedback_tiles[self.feedback_cursor] = Some(cur);
                }
            }
            Action::TileLeft if self.phase == InputPhase::SettingFeedback => {
                self.move_feedback_cursor(-1);
            }
            Action::TileRight if self.phase == InputPhase::SettingFeedback => {
                self.move_feedback_cursor(1);
            }
            _ => {}
        }
        false
    }

    /// Called each UI tick: poll async suggestion; for copilot, sync when ready.
    pub fn tick(&mut self) {
        let was_thinking = self.thinking;
        if self.poll_suggestion() && self.copilot && was_thinking {
            self.prepare_copilot_feedback();
        }
    }

    fn on_submit(&mut self) {
        match self.phase {
            InputPhase::TypingGuess => {
                let needed = self.editable_slots();
                if self.guess_buffer.len() != needed {
                    self.error = Some(format!(
                        "Enter {} letter{} for the remaining tiles",
                        needed,
                        if needed == 1 { "" } else { "s" }
                    ));
                    return;
                }
                let Some(word) = assemble_guess(&self.fixed_letters, &self.guess_buffer) else {
                    self.error = Some("Invalid word".into());
                    return;
                };
                self.constraint_warning = if !self.game.word_lists.is_valid_guess(word) {
                    Some(format!(
                        "'{word}' is not in our guess list — OK for NYT words we may be missing."
                    ))
                } else {
                    None
                };
                let _ = self.begin_feedback_phase(word);
            }
            InputPhase::SettingFeedback => {
                if self.thinking && self.copilot && self.pending_guess.is_none() {
                    self.error = Some("Still computing suggestion…".into());
                    return;
                }
                if self.feedback_tiles.iter().any(|t| t.is_none()) {
                    self.error = Some("Set feedback for all 5 tiles (g/y/x or Space)".into());
                    return;
                }
                let Some(guess) = self.active_guess() else {
                    self.error = Some("No active guess".into());
                    return;
                };
                let tiles: [Tile; 5] = self.feedback_tiles.map(|t| t.unwrap());
                let pattern = Pattern::new(tiles);

                let apply = if self.copilot {
                    self.game.apply_turn(guess, pattern)
                } else {
                    self.game.record_turn(guess, pattern)
                };

                match apply {
                    Ok(()) => {
                        self.error = None;
                        self.constraint_warning =
                            empty_candidate_warning(&self.game, guess, pattern);
                        self.guess_buffer.clear();
                        self.feedback_tiles = [None; 5];
                        self.feedback_cursor = 0;
                        self.request_suggestion();
                        self.persist_session();

                        if self.game.is_solved() || self.game.is_lost() {
                            self.begin_typing_phase();
                            self.thinking = false;
                        } else if self.copilot {
                            // Wait for async suggestion; tick will sync.
                            self.phase = InputPhase::SettingFeedback;
                            if !self.thinking {
                                self.prepare_copilot_feedback();
                            }
                        } else {
                            self.begin_typing_phase();
                        }
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
    }
}

pub fn empty_candidate_warning(game: &GameState, guess: Word, pattern: Pattern) -> Option<String> {
    if game.is_solved() || game.remaining_count() > 0 {
        return None;
    }

    let pool_only = guess_pool_only_matches(&game.word_lists, game.history().as_slice());
    if !pool_only.is_empty() {
        let sample: Vec<_> = pool_only.iter().take(5).map(|w| w.to_string()).collect();
        let extra = if pool_only.len() > sample.len() {
            format!(" (+{} more)", pool_only.len() - sample.len())
        } else {
            String::new()
        };
        return Some(format!(
            "No candidates in our answer list, but these guess-pool words match your history: \
             {}{}. NYT may use a word missing from answers.txt — try one of these, or run \
             scripts/update-wordlists.sh.",
            sample.join(", "),
            extra
        ));
    }

    let matches_any_answer = game
        .word_lists
        .answers
        .iter()
        .any(|&answer| compute_feedback(guess, answer) == pattern);

    if !matches_any_answer {
        return Some(format!(
            "No candidates — this feedback matches no word in our answer list ({guess}). \
             Check tile colors, or run scripts/update-wordlists.sh if today's NYT answer is missing."
        ));
    }

    Some(
        "No candidates — this feedback contradicts earlier turns. \
         Double-check tile colors (duplicate letters are easy to mis-enter)."
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordle_solver::core::words::shared_word_lists;
    use wordle_solver::OPENING_GUESS;

    #[test]
    fn begin_feedback_phase_rejects_hard_mode_violation() {
        let lists = shared_word_lists();
        let mut state = PlayState::new(
            lists,
            false,
            "Solver Aid",
            false,
            OPENING_GUESS,
            false,
            None,
        );
        state
            .game
            .record_turn(
                Word::parse("slate").unwrap(),
                Pattern::from_str("Gxxxx").unwrap(),
            )
            .unwrap();
        let bad = Word::parse("plate").unwrap();
        assert!(!state.begin_feedback_phase(bad));
        assert!(state.error.is_some());
    }

    #[test]
    fn solver_aid_shows_opening_suggestion_before_first_turn() {
        let lists = shared_word_lists();
        let state = PlayState::new(
            lists,
            false,
            "Solver Aid",
            false,
            OPENING_GUESS,
            false,
            None,
        );
        assert!(state.cached_suggestion().is_some());
        assert_eq!(state.phase, InputPhase::TypingGuess);
        assert!(!state.thinking);
    }

    #[test]
    fn copilot_starts_with_cached_opening_suggestion() {
        let lists = shared_word_lists();
        let state = PlayState::new(lists, true, "Copilot", false, OPENING_GUESS, false, None);
        assert!(state.cached_suggestion().is_some());
        assert!(state.active_guess().is_some());
        assert_eq!(state.phase, InputPhase::SettingFeedback);
    }

    #[test]
    fn async_suggestion_after_commit_and_stale_on_undo() {
        let lists = shared_word_lists();
        let mut state = PlayState::new(
            lists,
            false,
            "Solver Aid",
            false,
            OPENING_GUESS,
            false,
            None,
        );
        state
            .game
            .record_turn(
                Word::parse("slate").unwrap(),
                Pattern::from_str("xxxxx").unwrap(),
            )
            .unwrap();
        state.request_suggestion();
        assert!(state.thinking || state.cached_suggestion().is_some());

        // Wait for job.
        for _ in 0..500 {
            if state.poll_suggestion() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!state.thinking);
        assert!(state.cached_suggestion().is_some());
        let gen_before = state.suggestion_generation;

        state.game.undo_turn();
        state.invalidate_suggestion_job();
        assert!(state.suggestion_generation != gen_before || gen_before == 0);
        assert!(!state.thinking);
    }

    #[test]
    fn easy_mode_accepts_hard_mode_violating_guess() {
        let lists = shared_word_lists();
        let mut state =
            PlayState::new(lists, false, "Solver Aid", true, OPENING_GUESS, false, None);
        state
            .game
            .record_turn(
                Word::parse("slate").unwrap(),
                Pattern::from_str("Gxxxx").unwrap(),
            )
            .unwrap();
        assert!(state.begin_feedback_phase(Word::parse("plate").unwrap()));
    }

    #[test]
    fn colorblind_flag_toggles() {
        let lists = shared_word_lists();
        let mut state = PlayState::new(
            lists,
            false,
            "Solver Aid",
            false,
            OPENING_GUESS,
            false,
            None,
        );
        assert!(!state.colorblind);
        state.handle(Action::ToggleColorblind);
        assert!(state.colorblind);
    }
}
