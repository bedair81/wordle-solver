use std::io::{self, stdout};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
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
    aid, copilot, menu, InputPhase, MenuOption, MenuState, PlayState, MENU_OPTION_COUNT,
};

pub enum Screen {
    Menu(MenuState),
    Aid(PlayState),
    Copilot(PlayState),
}

pub struct App {
    pub screen: Screen,
    pub word_lists: Option<Arc<WordLists>>,
    load_rx: Receiver<Arc<WordLists>>,
    pub should_quit: bool,
}

impl App {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let lists = Arc::new(WordLists::load());
            let _ = tx.send(lists);
        });

        Self {
            screen: Screen::Menu(MenuState::new()),
            word_lists: None,
            load_rx: rx,
            should_quit: false,
        }
    }

    pub fn is_loading(&self) -> bool {
        self.word_lists.is_none()
    }

    fn poll_word_lists(&mut self) {
        if self.word_lists.is_none() {
            if let Ok(lists) = self.load_rx.try_recv() {
                self.word_lists = Some(lists);
            }
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
                        InputPhase::TypingGuess => InputContext::TypingWord {
                            has_turns: !state.game.turns.is_empty(),
                        },
                        InputPhase::SettingFeedback => InputContext::SettingFeedback,
                    }
                }
            }
        }
    }

    fn handle_action(&mut self, action: Action) {
        let word_lists = self.word_lists.clone();

        match &mut self.screen {
            Screen::Menu(state) => match action {
                Action::Quit => self.should_quit = true,
                Action::Up => state.move_up(),
                Action::Down => state.move_down(MENU_OPTION_COUNT),
                Action::Help => state.show_help = !state.show_help,
                Action::Submit => {
                    let Some(word_lists) = word_lists else {
                        return;
                    };
                    match menu::menu_option(state.selected) {
                        Some(MenuOption::SolverAid) => {
                            self.screen =
                                Screen::Aid(aid::PlayState::new(word_lists, false, "Solver Aid"));
                        }
                        Some(MenuOption::Copilot) => {
                            self.screen = Screen::Copilot(copilot::new(word_lists));
                        }
                        None => {}
                    }
                }
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
        }
    }

    fn tick(&mut self) {
        self.poll_word_lists();
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
    if app.is_loading() {
        menu::render_loading(frame);
        return;
    }

    match &mut app.screen {
        Screen::Menu(state) => menu::render(frame, state),
        Screen::Aid(state) => aid::render(frame, state),
        Screen::Copilot(state) => copilot::render(frame, state),
    }
}
