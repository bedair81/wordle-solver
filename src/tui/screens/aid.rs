//! Solver Aid / shared play rendering (state lives in `play_state`).

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

use wordle_solver::core::filter::EmptyCandidates;

use crate::tui::screens::play_state::{InputPhase, PlayState};
use crate::tui::theme;
use crate::tui::widgets::TileRow;

pub fn render(frame: &mut Frame, state: &mut PlayState) {
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

    let mode_tag = if state.game.easy_mode() {
        " [easy]"
    } else {
        " [hard]"
    };
    let cb_tag = if state.colorblind { " [CB]" } else { "" };
    let header = Paragraph::new(format!("{}{}{}", state.title, mode_tag, cb_tag))
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
                fixed_letters: None,
                feedback_draft: None,
                feedback_cursor: None,
                colorblind: state.colorblind,
            },
            row_area,
        );
    }
}

fn render_stats(frame: &mut Frame, state: &PlayState, area: ratatui::layout::Rect) {
    let suggestion = state.cached_suggestion();
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

    if state.thinking {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Suggested: computing…",
            theme::highlight_style(),
        )));
        if let Some(s) = suggestion {
            lines.push(Line::from(Span::styled(
                format!("  (previous: {})", s.word),
                theme::muted_style(),
            )));
        }
    } else if let Some(s) = suggestion {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Suggested: ", theme::muted_style()),
            Span::styled(s.word.to_string(), theme::highlight_style()),
        ]));
        let in_remaining = state.game.remaining_answers().contains(&s.word);
        if !in_remaining && state.game.remaining_count() > 0 {
            lines.push(Line::from(Span::styled(
                "  (split probe — not in candidate list)",
                theme::muted_style(),
            )));
        }
        lines.push(Line::from(vec![
            Span::styled("Score: ", theme::muted_style()),
            Span::raw(format!("{:.2}", s.entropy)),
        ]));
    } else if !state.game.is_solved() && !state.game.is_lost() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Suggested: — (no compliant guess; check feedback)",
            theme::error_style(),
        )));
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
    let title = format!("Candidates ({})", remaining.len());

    if remaining.is_empty() && !state.game.is_solved() {
        let message = if let Some(warning) = state.constraint_warning() {
            warning.to_string()
        } else {
            // No pending turn context — classify from last turn if present.
            empty_candidates_message(state)
        };
        let block = Paragraph::new(message)
            .wrap(Wrap { trim: true })
            .style(theme::error_style())
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
            );
        frame.render_widget(block, area);
        return;
    }

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
                if state.fixed_letters().iter().any(|slot| slot.is_some()) {
                    lines.push(Line::from(
                        "NYT hard mode: green tiles locked — type remaining letters (include yellows from prior turns):",
                    ));
                } else if !state.game.turns.is_empty() {
                    if state.game.easy_mode() {
                        lines.push(Line::from(
                            "Easy mode: type any guess, then Enter for feedback:",
                        ));
                    } else {
                        lines.push(Line::from(
                            "NYT hard mode: include all yellow letters from prior turns, then Enter for feedback:",
                        ));
                    }
                } else {
                    lines.push(Line::from(
                        "Type your guess, then Enter to set NYT feedback:",
                    ));
                }
            }
            InputPhase::SettingFeedback => {
                if state.thinking && state.is_copilot() {
                    lines.push(Line::from(Span::styled(
                        "Computing next suggestion… (Esc/q still work)",
                        theme::highlight_style(),
                    )));
                } else if state.is_copilot() {
                    lines.push(Line::from(
                        "Play the suggested word on NYT, then set tile colors (g/y/x):",
                    ));
                } else {
                    lines.push(Line::from(
                        "Set each tile to match NYT (g/y/x or Space), Enter to commit:",
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
                    fixed_letters: None,
                    feedback_draft: Some(state.feedback_tiles),
                    feedback_cursor: Some(state.feedback_cursor),
                    colorblind: state.colorblind,
                },
                row_area,
            );
        } else {
            let fixed = state.fixed_letters();
            frame.render_widget(
                TileRow {
                    word: None,
                    pattern: None,
                    buffer: Some(&state.guess_buffer),
                    fixed_letters: if fixed.iter().any(|s| s.is_some()) {
                        Some(fixed)
                    } else {
                        None
                    },
                    feedback_draft: None,
                    feedback_cursor: None,
                    colorblind: state.colorblind,
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
        let undo_reset = if state.phase == InputPhase::TypingGuess {
            ""
        } else {
            "u undo | r reset | "
        };
        return format!(
            "Mode: {} | c colorblind tiles | g/y/x or Space | ←/→ | Enter | {undo_reset}Esc back | q quit",
            if state.game.easy_mode() {
                "easy"
            } else {
                "hard"
            }
        );
    }
    if state.thinking {
        return "Computing suggestion… | Esc back | q quit | ? help".into();
    }
    if state.game.is_solved() || state.game.is_lost() {
        return "u undo | r reset | Esc back | q quit | ? help".into();
    }
    match state.phase {
        InputPhase::TypingGuess => {
            "Type guess | Enter next | ↑/↓ scroll | c colorblind | ? help".into()
        }
        InputPhase::SettingFeedback => {
            "g/y/x tiles | Enter commit | ←/→ | u undo | r reset | c colorblind | ? help".into()
        }
    }
}

fn empty_candidates_message(state: &PlayState) -> String {
    if let Some(last) = state.game.turns.last() {
        if let Some(status) = EmptyCandidates::classify(&state.game, last.guess, last.pattern) {
            return status.short_message();
        }
    }
    "No candidates match these constraints — check feedback or update word lists.".into()
}

#[cfg(test)]
mod tests {
    use wordle_solver::core::pattern::Tile;

    use crate::tui::theme::colorblind_mark;

    #[test]
    fn colorblind_symbols_distinct() {
        assert_ne!(
            colorblind_mark(Tile::Correct),
            colorblind_mark(Tile::Present)
        );
        assert_ne!(
            colorblind_mark(Tile::Present),
            colorblind_mark(Tile::Absent)
        );
    }
}
