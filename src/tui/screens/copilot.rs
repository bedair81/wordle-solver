use std::sync::Arc;

use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

use super::play_state::PlayState;

pub fn new(
    word_lists: Arc<WordLists>,
    easy_mode: bool,
    opening: Word,
    colorblind: bool,
    session_path: Option<std::path::PathBuf>,
) -> PlayState {
    PlayState::new(
        word_lists,
        true,
        "Copilot",
        easy_mode,
        opening,
        colorblind,
        session_path,
    )
}

pub fn render(frame: &mut ratatui::Frame, state: &mut PlayState) {
    super::aid::render(frame, state);
}

pub fn handle(state: &mut PlayState, action: crate::tui::input::Action) -> bool {
    state.handle(action)
}
