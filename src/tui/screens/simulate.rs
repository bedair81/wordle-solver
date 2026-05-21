use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};

use wordle_solver::core::solver::auto_solve;
use wordle_solver::core::word::Word;
use wordle_solver::core::words::WordLists;

use crate::tui::input::Action;
use crate::tui::theme;
use crate::tui::widgets::TileRow;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SimulateView {
    Menu,
    SingleSetup,
    SingleRunning,
    SingleDone,
    BenchmarkRunning,
    BenchmarkDone,
}

pub struct BenchmarkResult {
    pub total: usize,
    pub solved: usize,
    pub failed: usize,
    pub total_guesses: usize,
    pub distribution: [usize; 6],
    pub hardest: Vec<(Word, usize)>,
    pub elapsed: Duration,
}

pub enum BenchmarkMsg {
    Progress { done: usize, total: usize, running_avg: f64 },
    Done(BenchmarkResult),
}

pub struct SimulateState {
    pub view: SimulateView,
    pub menu_selected: usize,
    pub target_buffer: String,
    pub target: Option<Word>,
    pub animated_turns: Vec<(Word, wordle_solver::core::pattern::Pattern)>,
    pub anim_index: usize,
    pub last_tick: Instant,
    pub error: Option<String>,
    pub show_help: bool,
    pub benchmark_rx: Option<Receiver<BenchmarkMsg>>,
    pub benchmark_progress: (usize, usize, f64),
    pub benchmark_result: Option<BenchmarkResult>,
    pub cancel_benchmark: Option<Sender<()>>,
}

impl SimulateState {
    pub fn new() -> Self {
        Self {
            view: SimulateView::Menu,
            menu_selected: 0,
            target_buffer: String::new(),
            target: None,
            animated_turns: Vec::new(),
            anim_index: 0,
            last_tick: Instant::now(),
            error: None,
            show_help: false,
            benchmark_rx: None,
            benchmark_progress: (0, 0, 0.0),
            benchmark_result: None,
            cancel_benchmark: None,
        }
    }

    pub fn tick(&mut self, word_lists: &WordLists) {
        if self.view == SimulateView::SingleRunning {
            if self.last_tick.elapsed() >= Duration::from_millis(600) {
                self.last_tick = Instant::now();
                if self.anim_index < self.animated_turns.len() {
                    self.anim_index += 1;
                } else if self.anim_index == self.animated_turns.len() && !self.animated_turns.is_empty() {
                    self.view = SimulateView::SingleDone;
                }
            }
        }

        if self.view == SimulateView::BenchmarkRunning {
            let msg = self.benchmark_rx.as_ref().and_then(|rx| rx.try_recv().ok());
            if let Some(msg) = msg {
                match msg {
                    BenchmarkMsg::Progress { done, total, running_avg } => {
                        self.benchmark_progress = (done, total, running_avg);
                    }
                    BenchmarkMsg::Done(result) => {
                        self.benchmark_result = Some(result);
                        self.view = SimulateView::BenchmarkDone;
                        self.benchmark_rx = None;
                        self.cancel_benchmark = None;
                    }
                }
            }
        }

        let _ = word_lists;
    }

    pub fn handle(&mut self, action: Action, word_lists: Arc<WordLists>) -> bool {
        match self.view {
            SimulateView::Menu => match action {
                Action::Quit => return true,
                Action::Back => return true,
                Action::Help => self.show_help = !self.show_help,
                Action::Up => {
                    if self.menu_selected > 0 {
                        self.menu_selected -= 1;
                    }
                }
                Action::Down => {
                    if self.menu_selected < 1 {
                        self.menu_selected += 1;
                    }
                }
                Action::Submit => {
                    if self.menu_selected == 0 {
                        self.view = SimulateView::SingleSetup;
                        self.error = None;
                    } else {
                        self.start_benchmark(word_lists);
                    }
                }
                _ => {}
            },
            SimulateView::SingleSetup => match action {
                Action::Quit => return true,
                Action::Back => {
                    self.view = SimulateView::Menu;
                    self.target_buffer.clear();
                    self.error = None;
                }
                Action::Help => self.show_help = !self.show_help,
                Action::Char(c) if self.target_buffer.len() < 5 => {
                    self.target_buffer.push(c);
                    self.error = None;
                }
                Action::Delete => {
                    self.target_buffer.pop();
                }
                Action::Submit => {
                    if self.target_buffer.is_empty() {
                        let idx = simple_random(word_lists.answers.len());
                        self.target = Some(word_lists.answers[idx]);
                    } else if self.target_buffer.len() != 5 {
                        self.error = Some("Enter 5 letters or leave blank for random".into());
                        return false;
                    } else {
                        let Some(word) = Word::from_str(&self.target_buffer) else {
                            self.error = Some("Invalid word".into());
                            return false;
                        };
                        if !word_lists.is_answer(word) {
                            self.error = Some(format!("'{word}' is not in the answer list"));
                            return false;
                        }
                        self.target = Some(word);
                    }
                    if let Some(target) = self.target {
                        self.start_single(target, &word_lists);
                    }
                }
                _ => {}
            },
            SimulateView::SingleRunning | SimulateView::SingleDone => match action {
                Action::Quit => return true,
                Action::Back | Action::Reset => {
                    self.view = SimulateView::Menu;
                    self.target_buffer.clear();
                    self.target = None;
                    self.animated_turns.clear();
                    self.anim_index = 0;
                }
                Action::Help => self.show_help = !self.show_help,
                Action::Submit if self.view == SimulateView::SingleDone => {
                    self.view = SimulateView::SingleSetup;
                    self.target_buffer.clear();
                    self.target = None;
                    self.animated_turns.clear();
                    self.anim_index = 0;
                }
                _ => {}
            },
            SimulateView::BenchmarkRunning => match action {
                Action::Quit => return true,
                Action::Back => {
                    if let Some(tx) = self.cancel_benchmark.take() {
                        let _ = tx.send(());
                    }
                    self.view = SimulateView::Menu;
                    self.benchmark_rx = None;
                }
                Action::Help => self.show_help = !self.show_help,
                _ => {}
            },
            SimulateView::BenchmarkDone => match action {
                Action::Quit => return true,
                Action::Back | Action::Reset => {
                    self.view = SimulateView::Menu;
                    self.benchmark_result = None;
                }
                Action::Help => self.show_help = !self.show_help,
                _ => {}
            },
        }
        false
    }

    fn start_single(&mut self, target: Word, word_lists: &WordLists) {
        match auto_solve(word_lists, target) {
            Some(turns) => {
                self.animated_turns = turns;
                self.anim_index = 0;
                self.target = Some(target);
                self.view = SimulateView::SingleRunning;
                self.last_tick = Instant::now();
                self.error = None;
            }
            None => {
                self.error = Some(format!("Solver failed to solve '{target}' in 6 guesses"));
            }
        }
    }

    fn start_benchmark(&mut self, word_lists: Arc<WordLists>) {
        let (tx, rx) = mpsc::channel();
        let (cancel_tx, cancel_rx) = mpsc::channel();
        self.benchmark_rx = Some(rx);
        self.cancel_benchmark = Some(cancel_tx);
        self.benchmark_progress = (0, word_lists.answers.len(), 0.0);
        self.benchmark_result = None;
        self.view = SimulateView::BenchmarkRunning;
        self.error = None;

        thread::spawn(move || {
            let start = Instant::now();
            let total = word_lists.answers.len();
            let mut solved = 0usize;
            let mut failed = 0usize;
            let mut total_guesses = 0usize;
            let mut distribution = [0usize; 6];
            let mut hardest = Vec::new();

            for (i, &target) in word_lists.answers.iter().enumerate() {
                if cancel_rx.try_recv().is_ok() {
                    return;
                }

                match auto_solve(&word_lists, target) {
                    Some(history) => {
                        let n = history.len();
                        solved += 1;
                        total_guesses += n;
                        if n >= 1 && n <= 6 {
                            distribution[n - 1] += 1;
                        }
                        hardest.push((target, n));
                    }
                    None => failed += 1,
                }

                if (i + 1) % 25 == 0 || i + 1 == total {
                    let running_avg = if solved > 0 {
                        total_guesses as f64 / solved as f64
                    } else {
                        0.0
                    };
                    let _ = tx.send(BenchmarkMsg::Progress {
                        done: i + 1,
                        total,
                        running_avg,
                    });
                }
            }

            hardest.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            hardest.truncate(10);

            let _ = tx.send(BenchmarkMsg::Done(BenchmarkResult {
                total,
                solved,
                failed,
                total_guesses,
                distribution,
                hardest,
                elapsed: start.elapsed(),
            }));
        });
    }
}

fn simple_random(max: usize) -> usize {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    Instant::now().hash(&mut h);
    (h.finish() as usize) % max
}

const MENU: &[&str] = &["Single word simulation", "Full benchmark (all answers)"];

pub fn render(frame: &mut Frame, state: &SimulateState) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(ratatui::style::Style::default().bg(theme::BG)),
        area,
    );

    match state.view {
        SimulateView::Menu => render_menu(frame, state, area),
        SimulateView::SingleSetup => render_single_setup(frame, state, area),
        SimulateView::SingleRunning | SimulateView::SingleDone => {
            render_single_run(frame, state, area)
        }
        SimulateView::BenchmarkRunning => render_benchmark_running(frame, state, area),
        SimulateView::BenchmarkDone => render_benchmark_done(frame, state, area),
    }
}

fn render_menu(frame: &mut Frame, state: &SimulateState, area: ratatui::layout::Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(8), Constraint::Length(3)])
        .split(area);

    frame.render_widget(
        Paragraph::new("Simulate")
            .style(theme::title_style())
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::BOTTOM).border_style(ratatui::style::Style::default().fg(theme::BORDER))),
        chunks[0],
    );

    let items: Vec<ListItem> = MENU
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let marker = if i == state.menu_selected { ">" } else { " " };
            let style = if i == state.menu_selected {
                theme::highlight_style()
            } else {
                ratatui::style::Style::default().fg(theme::FG)
            };
            ListItem::new(Line::from(Span::styled(format!("{marker} {label}"), style)))
        })
        .collect();

    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
        ),
        chunks[1],
    );

    let footer = if state.show_help {
        "Single: watch solver vs one answer. Benchmark: test all ~2309 answers.\nEsc back | Enter select"
    } else {
        "↑/↓ navigate | Enter select | ? help | Esc back | q quit"
    };
    frame.render_widget(
        Paragraph::new(footer)
            .style(theme::muted_style())
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::TOP).border_style(ratatui::style::Style::default().fg(theme::BORDER))),
        chunks[2],
    );
}

fn render_single_setup(frame: &mut Frame, state: &SimulateState, area: ratatui::layout::Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(6), Constraint::Length(3)])
        .split(area);

    frame.render_widget(
        Paragraph::new("Single word simulation")
            .style(theme::title_style())
            .alignment(Alignment::Center),
        chunks[0],
    );

    let mut lines = vec![
        Line::from("Enter target answer (must be in NYT answer list),"),
        Line::from("or press Enter with empty input for a random word:"),
        Line::from(""),
    ];
    if let Some(err) = &state.error {
        lines.push(Line::from(Span::styled(err.clone(), theme::error_style())));
    }

    let p = Paragraph::new(lines);
    frame.render_widget(p, chunks[1]);

    let input_y = chunks[1].y + 3;
    frame.render_widget(
        TileRow {
            word: None,
            pattern: None,
            buffer: Some(&state.target_buffer),
            feedback_draft: None,
            feedback_cursor: None,
        },
        ratatui::layout::Rect {
            x: chunks[1].x,
            y: input_y,
            width: chunks[1].width,
            height: 1,
        },
    );

    frame.render_widget(
        Paragraph::new("Type target | Enter run | Esc back")
            .style(theme::muted_style())
            .alignment(Alignment::Center),
        chunks[2],
    );
}

fn render_single_run(frame: &mut Frame, state: &SimulateState, area: ratatui::layout::Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)])
        .split(area);

    let target = state
        .target
        .map(|w| w.to_string())
        .unwrap_or_else(|| "?".into());
    let status = if state.view == SimulateView::SingleDone {
        format!("Solved '{target}' in {} guesses", state.animated_turns.len())
    } else {
        format!("Solving '{target}'...")
    };

    frame.render_widget(
        Paragraph::new(status)
            .style(theme::title_style())
            .alignment(Alignment::Center),
        chunks[0],
    );

    let inner = chunks[1];
    for (i, (guess, pattern)) in state
        .animated_turns
        .iter()
        .enumerate()
        .take(state.anim_index)
    {
        let row = ratatui::layout::Rect {
            x: inner.x,
            y: inner.y + i as u16,
            width: inner.width,
            height: 1,
        };
        frame.render_widget(
            TileRow {
                word: Some(*guess),
                pattern: Some(*pattern),
                buffer: None,
                feedback_draft: None,
                feedback_cursor: None,
            },
            row,
        );
    }

    let footer = if state.view == SimulateView::SingleDone {
        "Enter play again | Esc menu | r reset"
    } else {
        "Animating solver..."
    };
    frame.render_widget(
        Paragraph::new(footer)
            .style(theme::muted_style())
            .alignment(Alignment::Center),
        chunks[2],
    );
}

fn render_benchmark_running(frame: &mut Frame, state: &SimulateState, area: ratatui::layout::Rect) {
    let (done, total, avg) = state.benchmark_progress;
    let pct = if total > 0 {
        (done as f64 / total as f64 * 100.0) as u16
    } else {
        0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Min(4), Constraint::Length(3)])
        .split(area);

    frame.render_widget(
        Paragraph::new("Benchmark running")
            .style(theme::title_style())
            .alignment(Alignment::Center),
        chunks[0],
    );

    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).border_style(ratatui::style::Style::default().fg(theme::BORDER)))
        .gauge_style(ratatui::style::Style::default().fg(theme::CORRECT))
        .percent(pct.min(100))
        .label(format!("{done}/{total} ({pct}%)"));
    frame.render_widget(gauge, chunks[1]);

    frame.render_widget(
        Paragraph::new(format!("Running average (solved): {avg:.3} guesses"))
            .alignment(Alignment::Center),
        chunks[2],
    );

    frame.render_widget(
        Paragraph::new("Esc cancel | q quit")
            .style(theme::muted_style())
            .alignment(Alignment::Center),
        chunks[3],
    );
}

fn render_benchmark_done(frame: &mut Frame, state: &SimulateState, area: ratatui::layout::Rect) {
    let Some(result) = &state.benchmark_result else {
        return;
    };

    let avg = if result.solved > 0 {
        result.total_guesses as f64 / result.solved as f64
    } else {
        0.0
    };

    let mut lines = vec![
        Line::from(Span::styled("Benchmark complete", theme::title_style())),
        Line::from(""),
        Line::from(format!(
            "Solved: {} / {}  |  Failed: {}  |  Avg: {avg:.3}",
            result.solved, result.total, result.failed
        )),
        Line::from(format!("Time: {:.1}s", result.elapsed.as_secs_f64())),
        Line::from(""),
        Line::from("Distribution (guesses → count):"),
    ];

    for (i, &count) in result.distribution.iter().enumerate() {
        lines.push(Line::from(format!("  {} guesses: {count}", i + 1)));
    }

    lines.push(Line::from(""));
    lines.push(Line::from("Hardest words:"));
    for (word, n) in &result.hardest {
        lines.push(Line::from(format!("  {word}: {n} guesses")));
    }

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(ratatui::style::Style::default().fg(theme::BORDER)),
            ),
        area,
    );
}
