use std::io::{self, stdout};
use std::sync::Arc;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use wordle_solver::core::words::WordLists;

use crate::tui::input::{map_key, Action, InputContext};
use crate::tui::screens::{
    aid, copilot, menu, simulate, InputPhase, MenuState, PlayState, SimulateState, SimulateView,
};

pub enum Screen {
    Menu(MenuState),
    Aid(PlayState),
    Copilot(PlayState),
    Simulate(SimulateState),
}

pub struct App {
    pub screen: Screen,
    pub word_lists: Arc<WordLists>,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::Menu(MenuState::new()),
            word_lists: Arc::new(WordLists::load()),
            should_quit: false,
        }
    }

    fn input_context(&self) -> InputContext {
        match &self.screen {
            Screen::Menu(_) => InputContext::Menu,
            Screen::Aid(state) | Screen::Copilot(state) => {
                if state.game.is_solved() || state.game.is_lost() {
                    InputContext::ViewOnly
                } else {
                    match state.phase {
                        InputPhase::TypingGuess => InputContext::TypingWord,
                        InputPhase::SettingFeedback => InputContext::SettingFeedback,
                    }
                }
            }
            Screen::Simulate(state) => match state.view {
                SimulateView::SingleSetup => InputContext::TypingWord,
                SimulateView::Menu => InputContext::Menu,
                SimulateView::SingleRunning
                | SimulateView::SingleDone
                | SimulateView::BenchmarkRunning
                | SimulateView::BenchmarkDone => InputContext::ViewOnly,
            },
        }
    }

    fn handle_action(&mut self, action: Action) {
        match &mut self.screen {
            Screen::Menu(state) => match action {
                Action::Quit => self.should_quit = true,
                Action::Up => state.move_up(),
                Action::Down => state.move_down(3),
                Action::Help => state.show_help = !state.show_help,
                Action::Submit => match state.selected {
                    0 => {
                        self.screen = Screen::Aid(aid::PlayState::new(
                            self.word_lists.clone(),
                            false,
                            "Solver Aid",
                        ));
                    }
                    1 => {
                        self.screen =
                            Screen::Copilot(copilot::new(self.word_lists.clone()));
                    }
                    2 => self.screen = Screen::Simulate(SimulateState::new()),
                    _ => {}
                },
                _ => {}
            },
            Screen::Aid(state) => {
                if state.handle(action) {
                    self.screen = Screen::Menu(MenuState::new());
                }
            }
            Screen::Copilot(state) => {
                if copilot::handle(state, action) {
                    self.screen = Screen::Menu(MenuState::new());
                }
            }
            Screen::Simulate(state) => {
                if state.handle(action, self.word_lists.clone()) {
                    self.screen = Screen::Menu(MenuState::new());
                }
            }
        }
    }

    fn tick(&mut self) {
        if let Screen::Simulate(state) = &mut self.screen {
            state.tick(&self.word_lists);
        }
    }
}

pub fn run() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new();
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = std::time::Instant::now();

    loop {
        terminal.draw(|frame| render(frame, &mut app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));

        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let ctx = app.input_context();
                    if let Some(action) = map_key(key, ctx) {
                        app.handle_action(action);
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.tick();
            last_tick = std::time::Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn render(frame: &mut ratatui::Frame, app: &mut App) {
    match &mut app.screen {
        Screen::Menu(state) => menu::render(frame, state),
        Screen::Aid(state) => aid::render(frame, state),
        Screen::Copilot(state) => copilot::render(frame, state),
        Screen::Simulate(state) => simulate::render(frame, state),
    }
}
