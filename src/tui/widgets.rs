use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::Widget,
};

use wordle_solver::core::pattern::{Pattern, Tile};
use wordle_solver::core::word::Word;

use crate::tui::theme::{self, tile_style};

pub struct TileRow<'a> {
    pub word: Option<Word>,
    pub pattern: Option<Pattern>,
    pub buffer: Option<&'a str>,
    /// Hard-mode letters locked at each position while typing a guess.
    pub fixed_letters: Option<[Option<u8>; 5]>,
    pub feedback_draft: Option<[Option<Tile>; 5]>,
    pub feedback_cursor: Option<usize>,
}

impl TileRow<'_> {
    fn letter_at(&self, i: usize) -> char {
        if let Some(word) = self.word {
            return word.as_str().chars().nth(i).unwrap_or(' ');
        }

        if let Some(fixed) = self.fixed_letters {
            if let Some(b) = fixed[i] {
                return b as char;
            }
            if let Some(buffer) = self.buffer {
                let editable: Vec<usize> = (0..5).filter(|&j| fixed[j].is_none()).collect();
                if let Some(buf_idx) = editable.iter().position(|&j| j == i) {
                    return buffer.chars().nth(buf_idx).unwrap_or(' ');
                }
            }
            return ' ';
        }

        if let Some(buf_str) = self.buffer {
            return buf_str.chars().nth(i).unwrap_or(' ').to_ascii_uppercase();
        }

        ' '
    }

    fn tile_at(&self, i: usize) -> Tile {
        if let Some(pattern) = self.pattern {
            pattern.tiles[i]
        } else if let Some(draft) = self.feedback_draft {
            draft[i].unwrap_or(Tile::Absent)
        } else if self
            .fixed_letters
            .and_then(|fixed| fixed[i])
            .is_some()
        {
            Tile::Correct
        } else {
            Tile::Absent
        }
    }
}

impl Widget for TileRow<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cell_width = 3;
        let gap = 1;
        let total_width = cell_width * 5 + gap * 4;
        let start_x = area.x + (area.width.saturating_sub(total_width)) / 2;

        for i in 0..5 {
            let x = start_x + (i as u16) * (cell_width + gap as u16);
            if x + cell_width > area.right() {
                break;
            }

            let ch = self.letter_at(i);
            let tile = self.tile_at(i);
            let focused = self.feedback_cursor == Some(i);
            let styled = self.pattern.is_some()
                || self.feedback_draft.is_some()
                || self.fixed_letters.is_some();

            let style = if styled {
                tile_style(tile, focused)
            } else {
                Style::default()
                    .fg(theme::FG)
                    .bg(theme::BORDER)
                    .add_modifier(ratatui::style::Modifier::BOLD)
            };

            let label = if ch == ' ' {
                " ".repeat(cell_width as usize)
            } else {
                format!(" {} ", ch.to_ascii_uppercase())
            };

            let cell_area = Rect {
                x,
                y: area.y,
                width: cell_width,
                height: 1,
            };
            buf.set_string(cell_area.x, cell_area.y, label, style);
        }
    }
}
