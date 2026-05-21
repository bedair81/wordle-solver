use std::sync::Arc;

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use wordle_solver::core::game::GameState;
use wordle_solver::core::pattern::{Pattern, Tile};
use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

use crate::tui::input::Action;
use crate::tui::theme;
use crate::tui::widgets::TileRow;

#[derive(Clone, Copy, PartialEq, Eq)]
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
}

impl PlayState {
    pub fn new(word_lists: Arc<WordLists>, copilot: bool, title: &'static str) -> Self {
        Self {
            game: GameState::new(word_lists, false),
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
        }
    }

    fn suggested_word(&self) -> Option<Word> {
        self.game.suggest_next().map(|s| s.word)
    }

    fn active_guess(&self) -> Option<Word> {
        if self.copilot {
            self.suggested_word()
        } else if self.guess_buffer.len() == 5 {
            Word::from_str(&self.guess_buffer)
        } else {
            None
        }
    }

    fn prepare_copilot_feedback(&mut self) {
        if let Some(word) = self.suggested_word() {
            self.guess_buffer = word.as_str().to_string();
            self.feedback_tiles = [None; 5];
            self.feedback_cursor = 0;
            self.phase = InputPhase::SettingFeedback;
        }
    }

    pub fn handle(&mut self, action: Action) -> bool {
        match action {
            Action::Quit => return true,
            Action::Back => return true,
            Action::Help => {
                self.show_help = !self.show_help;
            }
            Action::ToggleHardMode => {
                self.game.toggle_hard_mode();
                self.error = None;
            }
            Action::Undo => {
                if self.game.undo_turn() {
                    self.error = None;
                    if self.copilot {
                        self.prepare_copilot_feedback();
                    } else {
                        self.phase = InputPhase::TypingGuess;
                        self.guess_buffer.clear();
                    }
                }
            }
            Action::Reset => {
                self.game.reset();
                self.guess_buffer.clear();
                self.feedback_tiles = [None; 5];
                self.feedback_cursor = 0;
                self.error = None;
                self.list_scroll = 0;
                self.phase = if self.copilot {
                    InputPhase::SettingFeedback
                } else {
                    InputPhase::TypingGuess
                };
                if self.copilot {
                    self.prepare_copilot_feedback();
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
                if self.guess_buffer.len() < 5 {
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
                self.feedback_tiles[self.feedback_cursor] = Some(Tile::Correct);
            }
            Action::SetTilePresent if self.phase == InputPhase::SettingFeedback => {
                self.feedback_tiles[self.feedback_cursor] = Some(Tile::Present);
            }
            Action::SetTileAbsent if self.phase == InputPhase::SettingFeedback => {
                self.feedback_tiles[self.feedback_cursor] = Some(Tile::Absent);
            }
            Action::CycleTile if self.phase == InputPhase::SettingFeedback => {
                let cur = self.feedback_tiles[self.feedback_cursor]
                    .unwrap_or(Tile::Absent)
                    .cycle();
                self.feedback_tiles[self.feedback_cursor] = Some(cur);
            }
            Action::TileLeft if self.phase == InputPhase::SettingFeedback => {
                self.feedback_cursor = self.feedback_cursor.saturating_sub(1);
            }
            Action::TileRight if self.phase == InputPhase::SettingFeedback => {
                if self.feedback_cursor < 4 {
                    self.feedback_cursor += 1;
                }
            }
            _ => {}
        }
        false
    }

    fn on_submit(&mut self) {
        match self.phase {
            InputPhase::TypingGuess => {
                if self.guess_buffer.len() != 5 {
                    self.error = Some("Enter a 5-letter guess".into());
                    return;
                }
                let Some(word) = Word::from_str(&self.guess_buffer) else {
                    self.error = Some("Invalid word".into());
                    return;
                };
                if !self.game.word_lists.is_valid_guess(word) {
                    self.error = Some(format!("'{word}' is not in the Wordle dictionary"));
                    return;
                }
                self.feedback_tiles = [None; 5];
                self.feedback_cursor = 0;
                self.phase = InputPhase::SettingFeedback;
            }
            InputPhase::SettingFeedback => {
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

                match self.game.apply_turn(guess, pattern) {
                    Ok(()) => {
                        self.error = None;
                        self.guess_buffer.clear();
                        self.feedback_tiles = [None; 5];
                        self.feedback_cursor = 0;

                        if self.game.is_solved() || self.game.is_lost() {
                            self.phase = InputPhase::TypingGuess;
                        } else if self.copilot {
                            self.prepare_copilot_feedback();
                        } else {
                            self.phase = InputPhase::TypingGuess;
                        }
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }
    }

    pub fn ensure_copilot_ready(&mut self) {
        if self.copilot && self.phase == InputPhase::SettingFeedback && self.guess_buffer.is_empty() {
            self.prepare_copilot_feedback();
        }
    }
}

pub fn render(frame: &mut Frame, state: &mut PlayState) {
    state.ensure_copilot_ready();

    let area = frame.area();
    frame.render_widget(
        Block::default().style(ratatui::style::Style::default().bg(theme::BG)),
        area,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(9),
            Constraint::Min(6),
            Constraint::Length(5),
            Constraint::Length(3),
        ])
        .split(area);

    let hard = if state.game.hard_mode { "ON" } else { "OFF" };
    let header = Paragraph::new(format!("{}  |  Hard mode: {hard}", state.title))
        .style(theme::title_style())
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
        );
    frame.render_widget(header, chunks[0]);

    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
        .split(chunks[1]);

    render_history(frame, state, top[0]);
    render_stats(frame, state, top[1]);
    render_candidates(frame, state, chunks[2]);
    render_input(frame, state, chunks[3]);

    let footer = footer_text(state);
    frame.render_widget(
        Paragraph::new(footer)
            .wrap(Wrap { trim: true })
            .style(theme::muted_style())
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
            ),
        chunks[4],
    );
}

fn render_history(frame: &mut Frame, state: &PlayState, area: ratatui::layout::Rect) {
    let block = Block::default()
        .title("Turns")
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme::BORDER));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    for (i, turn) in state.game.turns.iter().enumerate().take(6) {
        if i as u16 >= inner.height {
            break;
        }
        let row_area = ratatui::layout::Rect {
            x: inner.x,
            y: inner.y + i as u16,
            width: inner.width,
            height: 1,
        };
        frame.render_widget(
            TileRow {
                word: Some(turn.guess),
                pattern: Some(turn.pattern),
                buffer: None,
                feedback_draft: None,
                feedback_cursor: None,
            },
            row_area,
        );
    }
}

fn render_stats(frame: &mut Frame, state: &PlayState, area: ratatui::layout::Rect) {
    let suggestion = state.game.suggest_next();
    let status = if state.game.is_solved() {
        "Solved!".to_string()
    } else if state.game.is_lost() {
        "Out of guesses".to_string()
    } else {
        format!("Turn {}/6", state.game.turns.len() + 1)
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Remaining: ", theme::muted_style()),
            Span::styled(
                state.game.remaining_count().to_string(),
                theme::highlight_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("Status: ", theme::muted_style()),
            Span::raw(status),
        ]),
    ];

    if let Some(s) = suggestion {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Suggested: ", theme::muted_style()),
            Span::styled(s.word.to_string(), theme::highlight_style()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Entropy: ", theme::muted_style()),
            Span::raw(format!("{:.2}", s.entropy)),
        ]));
    }

    let block = Paragraph::new(lines).block(
        Block::default()
            .title("Stats")
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
    );
    frame.render_widget(block, area);
}

fn render_candidates(frame: &mut Frame, state: &PlayState, area: ratatui::layout::Rect) {
    let remaining = state.game.remaining_answers();
    let visible_height = area.height.saturating_sub(2) as usize;
    let start = state.list_scroll.min(remaining.len().saturating_sub(1));
    let end = (start + visible_height).min(remaining.len());

    let items: Vec<ListItem> = remaining[start..end]
        .iter()
        .map(|w| ListItem::new(w.as_str().to_uppercase()))
        .collect();

    let title = format!("Candidates ({})", remaining.len());
    let list = List::new(items).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
    );
    frame.render_widget(list, area);
}

fn render_input(frame: &mut Frame, state: &PlayState, area: ratatui::layout::Rect) {
    let mut lines = Vec::new();

    if state.game.is_solved() {
        lines.push(Line::from(Span::styled(
            "You solved it! Press r to reset or Esc to go back.",
            theme::highlight_style(),
        )));
    } else if state.game.is_lost() {
        let answers: Vec<_> = state
            .game
            .remaining_answers()
            .iter()
            .take(5)
            .map(|w| w.to_string())
            .collect();
        lines.push(Line::from(Span::styled(
            format!("Game over. Possible answers: {}", answers.join(", ")),
            theme::error_style(),
        )));
    } else {
        match state.phase {
            InputPhase::TypingGuess => {
                lines.push(Line::from("Type your guess, then Enter to set NYT feedback:"));
            }
            InputPhase::SettingFeedback => {
                if state.copilot {
                    lines.push(Line::from(
                        "Play the suggested word on NYT, then set tile colors (g/y/x):",
                    ));
                } else {
                    lines.push(Line::from(
                        "Set each tile to match NYT (g=green y=yellow x=gray), Enter to commit:",
                    ));
                }
            }
        }

        let row_area = ratatui::layout::Rect {
            x: area.x + 1,
            y: area.y + lines.len() as u16,
            width: area.width.saturating_sub(2),
            height: 1,
        };

        if state.phase == InputPhase::SettingFeedback {
            frame.render_widget(
                TileRow {
                    word: state.active_guess(),
                    pattern: None,
                    buffer: None,
                    feedback_draft: Some(state.feedback_tiles),
                    feedback_cursor: Some(state.feedback_cursor),
                },
                row_area,
            );
        } else {
            frame.render_widget(
                TileRow {
                    word: None,
                    pattern: None,
                    buffer: Some(&state.guess_buffer),
                    feedback_draft: None,
                    feedback_cursor: None,
                },
                row_area,
            );
        }
    }

    if let Some(err) = &state.error {
        lines.push(Line::from(Span::styled(err.clone(), theme::error_style())));
    }

    let block = Paragraph::new(lines).block(
        Block::default()
            .title("Input")
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
    );
    frame.render_widget(block, area);
}

fn footer_text(state: &PlayState) -> String {
    if state.show_help {
        return "g/y/x or Space cycle tiles | ←/→ move cursor | Enter commit | u undo | r reset | h hard mode | Esc back | q quit".into();
    }
    if state.game.is_solved() || state.game.is_lost() {
        return "r reset | Esc back | q quit | ? help".into();
    }
    match state.phase {
        InputPhase::TypingGuess => {
            "Type guess | Enter next | ↑/↓ scroll candidates | ? help | Esc back".into()
        }
        InputPhase::SettingFeedback => {
            "g/y/x tiles | Enter commit | ←/→ cursor | h hard | u undo | r reset | ? help".into()
        }
    }
}
