use ratatui::style::{Color, Modifier, Style};

pub const BG: Color = Color::Rgb(18, 18, 19);
pub const FG: Color = Color::Rgb(215, 218, 220);
pub const BORDER: Color = Color::Rgb(58, 58, 60);
pub const CORRECT: Color = Color::Rgb(106, 170, 100);
pub const PRESENT: Color = Color::Rgb(201, 180, 88);
pub const ABSENT: Color = Color::Rgb(120, 124, 126);
pub const HIGHLIGHT: Color = Color::Rgb(86, 156, 214);

use wordle_solver::core::pattern::Tile;

pub fn tile_style(tile: Tile, focused: bool) -> Style {
    let bg = match tile {
        Tile::Correct => CORRECT,
        Tile::Present => PRESENT,
        Tile::Absent => ABSENT,
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
