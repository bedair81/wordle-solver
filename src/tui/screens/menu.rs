use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use crate::tui::theme;

pub struct MenuState {
    pub selected: usize,
    pub show_help: bool,
}

impl MenuState {
    pub fn new() -> Self {
        Self {
            selected: 0,
            show_help: false,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self, max: usize) {
        if self.selected + 1 < max {
            self.selected += 1;
        }
    }
}

const OPTIONS: &[(&str, &str)] = &[
    ("Solver Aid", "Enter your guesses + NYT feedback; filter answers"),
    ("Copilot", "Solver picks guesses; you enter feedback from NYT"),
    ("Simulate", "Autonomous play: single word or full benchmark"),
];

pub fn render(frame: &mut Frame, state: &MenuState) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(ratatui::style::Style::default().bg(theme::BG)),
        area,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

    let title = Paragraph::new("NYTimes Wordle Solver")
        .style(theme::title_style())
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
        );
    frame.render_widget(title, chunks[0]);

    let items: Vec<ListItem> = OPTIONS
        .iter()
        .enumerate()
        .map(|(i, (name, desc))| {
            let marker = if i == state.selected { ">" } else { " " };
            let style = if i == state.selected {
                theme::highlight_style()
            } else {
                ratatui::style::Style::default().fg(theme::FG)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{marker} {name}"), style.add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::styled(*desc, theme::muted_style()),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title("Select mode")
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
    );
    frame.render_widget(list, chunks[1]);

    let footer = if state.show_help {
        Paragraph::new(
            "Solver Aid: manual guesses. Copilot: auto suggestions. Simulate: benchmark.\n\
             Enter select | q quit | Esc back",
        )
        .style(theme::muted_style())
    } else {
        Paragraph::new("↑/↓ navigate | Enter select | ? help | q quit")
            .style(theme::muted_style())
    };
    frame.render_widget(
        footer.alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
        ),
        chunks[2],
    );
}
