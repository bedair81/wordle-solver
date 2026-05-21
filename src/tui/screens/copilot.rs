use std::sync::Arc;

use wordle_solver::core::words::WordLists;

use super::aid::{self, PlayState};

pub fn new(word_lists: Arc<WordLists>) -> PlayState {
    PlayState::new(word_lists, true, "Copilot")
}

pub fn render(frame: &mut ratatui::Frame, state: &mut PlayState) {
    aid::render(frame, state);
}

pub fn handle(state: &mut PlayState, action: crate::tui::input::Action) -> bool {
    state.handle(action)
}
