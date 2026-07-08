use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    ToggleColorblind,
}

/// Controls which keys are interpreted as shortcuts vs typed input.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InputContext {
    Menu,
    /// Typing a 5-letter guess (Solver Aid). All letters type into the word; u/r are not shortcuts here.
    TypingWord,
    /// Setting NYT tile feedback colors.
    SettingFeedback,
    /// Read-only / results screens (shortcuts only, no typing).
    ViewOnly,
}

impl InputContext {
    fn allows_typing(self) -> bool {
        matches!(self, InputContext::TypingWord)
    }

    fn allows_tile_keys(self) -> bool {
        matches!(self, InputContext::SettingFeedback)
    }

    fn allows_play_shortcuts(self) -> bool {
        matches!(self, InputContext::SettingFeedback | InputContext::ViewOnly)
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
        // Colorblind toggle: 'c' outside typing; Ctrl is quit.
        KeyCode::Char('c')
            if event.modifiers.is_empty() && !ctx.allows_typing() && !ctx.allows_tile_keys() =>
        {
            Some(Action::ToggleColorblind)
        }
        KeyCode::Char('c') if event.modifiers.is_empty() && ctx.allows_play_shortcuts() => {
            Some(Action::ToggleColorblind)
        }
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
    fn typing_word_accepts_shortcut_letters_as_input() {
        for c in ['r', 'u', 'g'] {
            assert!(
                matches!(map_key(key(c), InputContext::TypingWord), Some(Action::Char(l)) if l == c),
                "{c} should type as a letter while entering a guess"
            );
        }
    }

    #[test]
    fn typing_word_accepts_h_as_letter() {
        assert!(matches!(
            map_key(key('h'), InputContext::TypingWord),
            Some(Action::Char('h'))
        ));
        assert!(map_key(key('h'), InputContext::SettingFeedback).is_none());
    }

    #[test]
    fn undo_reset_only_in_feedback_not_while_typing() {
        assert!(matches!(
            map_key(key('u'), InputContext::TypingWord),
            Some(Action::Char('u'))
        ));
        assert!(matches!(
            map_key(key('u'), InputContext::SettingFeedback),
            Some(Action::Undo)
        ));
        assert!(matches!(
            map_key(key('r'), InputContext::TypingWord),
            Some(Action::Char('r'))
        ));
        assert!(matches!(
            map_key(key('r'), InputContext::SettingFeedback),
            Some(Action::Reset)
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

    #[test]
    fn colorblind_toggle_in_feedback_and_menu() {
        assert!(matches!(
            map_key(key('c'), InputContext::SettingFeedback),
            Some(Action::ToggleColorblind)
        ));
        assert!(matches!(
            map_key(key('c'), InputContext::Menu),
            Some(Action::ToggleColorblind)
        ));
        assert!(matches!(
            map_key(key('c'), InputContext::TypingWord),
            Some(Action::Char('c'))
        ));
    }
}
