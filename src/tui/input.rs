use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    Back,
    Up,
    Down,
    Help,
    Undo,
    Reset,
    Char(char),
    Delete,
    Submit,
    SetTileCorrect,
    SetTilePresent,
    SetTileAbsent,
    CycleTile,
    TileLeft,
    TileRight,
}

/// Controls which keys are interpreted as shortcuts vs typed input.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InputContext {
    Menu,
    /// Typing a 5-letter guess (Solver Aid). Undo/reset available when `has_turns`.
    TypingWord {
        has_turns: bool,
    },
    /// Setting NYT tile feedback colors.
    SettingFeedback,
    /// Read-only / results screens (shortcuts only, no typing).
    ViewOnly,
}

impl InputContext {
    fn allows_typing(self) -> bool {
        matches!(self, InputContext::TypingWord { .. })
    }

    fn allows_tile_keys(self) -> bool {
        matches!(self, InputContext::SettingFeedback)
    }

    fn allows_play_shortcuts(self) -> bool {
        match self {
            InputContext::SettingFeedback | InputContext::ViewOnly => true,
            InputContext::TypingWord { has_turns } => has_turns,
            InputContext::Menu => false,
        }
    }
}

pub fn map_key(event: KeyEvent, ctx: InputContext) -> Option<Action> {
    match event.code {
        KeyCode::Char('q') if event.modifiers.is_empty() => Some(Action::Quit),
        KeyCode::Char('c') if event.modifiers.contains(KeyModifiers::CONTROL) => Some(Action::Quit),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Up => Some(Action::Up),
        KeyCode::Down => Some(Action::Down),
        KeyCode::Enter => Some(Action::Submit),
        KeyCode::Char('?') => Some(Action::Help),
        KeyCode::Char('u') if ctx.allows_play_shortcuts() => Some(Action::Undo),
        KeyCode::Char('r') if ctx.allows_play_shortcuts() => Some(Action::Reset),
        KeyCode::Backspace if ctx.allows_typing() => Some(Action::Delete),
        KeyCode::Left if ctx.allows_tile_keys() => Some(Action::TileLeft),
        KeyCode::Right if ctx.allows_tile_keys() => Some(Action::TileRight),
        KeyCode::Char('g') if ctx.allows_tile_keys() => Some(Action::SetTileCorrect),
        KeyCode::Char('y') if ctx.allows_tile_keys() => Some(Action::SetTilePresent),
        KeyCode::Char('x') if ctx.allows_tile_keys() => Some(Action::SetTileAbsent),
        KeyCode::Char(' ') if ctx.allows_tile_keys() => Some(Action::CycleTile),
        KeyCode::Char(c) if c.is_ascii_alphabetic() && ctx.allows_typing() => {
            Some(Action::Char(c.to_ascii_lowercase()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    fn key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    #[test]
    fn typing_word_accepts_r_and_g_without_turns() {
        assert!(matches!(
            map_key(key('r'), InputContext::TypingWord { has_turns: false }),
            Some(Action::Char('r'))
        ));
        assert!(matches!(
            map_key(key('g'), InputContext::TypingWord { has_turns: false }),
            Some(Action::Char('g'))
        ));
    }

    #[test]
    fn typing_word_with_turns_maps_r_to_reset() {
        assert!(matches!(
            map_key(key('r'), InputContext::TypingWord { has_turns: true }),
            Some(Action::Reset)
        ));
    }

    #[test]
    fn typing_word_accepts_h_as_letter() {
        assert!(matches!(
            map_key(key('h'), InputContext::TypingWord { has_turns: false }),
            Some(Action::Char('h'))
        ));
        assert!(matches!(
            map_key(key('h'), InputContext::SettingFeedback),
            None
        ));
    }

    #[test]
    fn feedback_mode_maps_g_to_correct() {
        assert!(matches!(
            map_key(key('g'), InputContext::SettingFeedback),
            Some(Action::SetTileCorrect)
        ));
        assert!(matches!(
            map_key(key('r'), InputContext::SettingFeedback),
            Some(Action::Reset)
        ));
    }
}
