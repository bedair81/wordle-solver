use ratatui::style::{Color, Modifier, Style};

use crate::core::pattern::Tile;

pub const BG: Color = Color::Rgb(18, 18, 19);
pub const FG: Color = Color::Rgb(215, 218, 220);
pub const BORDER: Color = Color::Rgb(58, 58, 60);
pub const CORRECT: Color = Color::Rgb(106, 170, 100);
pub const PRESENT: Color = Color::Rgb(201, 180, 88);
pub const ABSENT: Color = Color::Rgb(120, 124, 126);
pub const HIGHLIGHT: Color = Color::Rgb(86, 156, 214);

// High-contrast palette for colorblind mode (blue / orange / gray).
pub const CB_CORRECT: Color = Color::Rgb(0, 114, 178);
pub const CB_PRESENT: Color = Color::Rgb(230, 159, 0);
pub const CB_ABSENT: Color = Color::Rgb(86, 86, 86);

pub fn tile_style(tile: Tile, focused: bool, colorblind: bool) -> Style {
    let bg = if colorblind {
        match tile {
            Tile::Correct => CB_CORRECT,
            Tile::Present => CB_PRESENT,
            Tile::Absent => CB_ABSENT,
        }
    } else {
        match tile {
            Tile::Correct => CORRECT,
            Tile::Present => PRESENT,
            Tile::Absent => ABSENT,
        }
    };
    let mut style = Style::default()
        .fg(Color::White)
        .bg(bg)
        .add_modifier(Modifier::BOLD);
    if focused {
        style = style.add_modifier(Modifier::REVERSED);
    }
    style
}

pub fn title_style() -> Style {
    Style::default().fg(FG).add_modifier(Modifier::BOLD)
}

pub fn muted_style() -> Style {
    Style::default().fg(Color::Rgb(150, 152, 155))
}

pub fn error_style() -> Style {
    Style::default().fg(Color::Rgb(220, 80, 80))
}

pub fn highlight_style() -> Style {
    Style::default().fg(HIGHLIGHT).add_modifier(Modifier::BOLD)
}

/// Symbol used beside the letter in colorblind mode.
pub fn colorblind_mark(tile: Tile) -> char {
    match tile {
        Tile::Correct => '■',
        Tile::Present => '▲',
        Tile::Absent => '·',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorblind_styles_differ_from_default_palette() {
        // Ensure CB colors are not identical to default green/yellow.
        assert_ne!(CB_CORRECT, CORRECT);
        assert_ne!(CB_PRESENT, PRESENT);
        let s_cb = tile_style(Tile::Correct, false, true);
        let s_def = tile_style(Tile::Correct, false, false);
        assert_ne!(s_cb.bg, s_def.bg);
    }
}
