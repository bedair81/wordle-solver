use std::io;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};

use crate::core::config::AppConfig;
use crate::core::session::{config_from_snapshot, load_session, restore_into_game};
use crate::core::words::{load_word_lists, WordLists};

use crate::tui::input::{map_key, Action, InputContext};
use crate::tui::screens::{
    aid, copilot, menu, InputPhase, MenuOption, MenuState, PlayState, MENU_OPTION_COUNT,
};
use crate::tui::terminal_guard::TerminalGuard;

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
    pub config: AppConfig,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let (tx, rx) = mpsc::channel();
        let cfg = config.clone();
        thread::spawn(move || {
            let lists = Arc::new(load_word_lists(&cfg));
            let _ = tx.send(lists);
        });

        Self {
            screen: Screen::Menu(MenuState::new()),
            word_lists: None,
            load_rx: rx,
            should_quit: false,
            config,
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
                        InputPhase::TypingGuess => InputContext::TypingWord,
                        InputPhase::SettingFeedback => InputContext::SettingFeedback,
                    }
                }
            }
        }
    }

    fn open_play(&mut self, copilot: bool) {
        let Some(word_lists) = self.word_lists.clone() else {
            return;
        };
        let session_path = Some(self.config.resolve_session_path());

        let mut restored = None;
        if let Ok(Some(snap)) = load_session(&self.config.resolve_session_path()) {
            if snap.copilot == copilot {
                self.config = config_from_snapshot(&self.config, &snap);
                restored = Some(snap);
            }
        }

        let mut state = if copilot {
            copilot::new(
                Arc::clone(&word_lists),
                self.config.easy_mode,
                self.config.opening,
                self.config.colorblind,
                session_path,
            )
        } else {
            PlayState::new(
                word_lists,
                false,
                "Solver Aid",
                self.config.easy_mode,
                self.config.opening,
                self.config.colorblind,
                session_path,
            )
        };

        if let Some(snap) = restored {
            if restore_into_game(&mut state.game, &snap).is_ok() {
                state.colorblind = snap.colorblind;
                state.after_session_restore();
            }
        }

        self.screen = if copilot {
            Screen::Copilot(state)
        } else {
            Screen::Aid(state)
        };
    }

    fn handle_action(&mut self, action: Action) {
        match &mut self.screen {
            Screen::Menu(state) => match action {
                Action::Quit => self.should_quit = true,
                Action::Up => state.move_up(),
                Action::Down => state.move_down(MENU_OPTION_COUNT),
                Action::Help => state.show_help = !state.show_help,
                Action::ToggleColorblind => {
                    self.config.colorblind = !self.config.colorblind;
                }
                Action::Submit => match menu::menu_option(state.selected) {
                    Some(MenuOption::SolverAid) => self.open_play(false),
                    Some(MenuOption::Copilot) => self.open_play(true),
                    None => {}
                },
                _ => {}
            },
            Screen::Aid(state) => {
                if matches!(action, Action::ToggleColorblind) {
                    self.config.colorblind = !self.config.colorblind;
                }
                if state.handle(action) {
                    self.screen = Screen::Menu(MenuState::new());
                }
            }
            Screen::Copilot(state) => {
                if matches!(action, Action::ToggleColorblind) {
                    self.config.colorblind = !self.config.colorblind;
                }
                if copilot::handle(state, action) {
                    self.screen = Screen::Menu(MenuState::new());
                }
            }
        }
    }

    fn tick(&mut self) {
        self.poll_word_lists();
        match &mut self.screen {
            Screen::Aid(state) | Screen::Copilot(state) => state.tick(),
            Screen::Menu(_) => {}
        }
    }
}

pub fn run(config: AppConfig) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    let mut app = App::new(config);
    let tick_rate = Duration::from_millis(50);
    let mut last_tick = std::time::Instant::now();

    loop {
        guard.terminal().draw(|frame| render(frame, &mut app))?;

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or(Duration::from_secs(0));

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

    guard.leave()?;
    Ok(())
}

fn render(frame: &mut ratatui::Frame, app: &mut App) {
    if app.is_loading() {
        menu::render_loading(frame);
        return;
    }

    match &mut app.screen {
        Screen::Menu(state) => menu::render(frame, state, &app.config),
        Screen::Aid(state) => aid::render(frame, state),
        Screen::Copilot(state) => copilot::render(frame, state),
    }
}
