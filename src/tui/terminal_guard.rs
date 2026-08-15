//! Ensures raw mode / alternate screen are restored on panic or Drop.

use std::io::{self, stdout, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

static HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
static NEEDS_RESTORE: AtomicBool = AtomicBool::new(false);

/// Install a panic hook that restores the terminal, chaining to the previous hook.
pub fn install_panic_hook() {
    if HOOK_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal_now();
        prev(info);
    }));
}

fn restore_terminal_now() -> io::Result<()> {
    if NEEDS_RESTORE.swap(false, Ordering::SeqCst) {
        let _ = disable_raw_mode();
        let mut out = stdout();
        let _ = execute!(out, LeaveAlternateScreen);
        let _ = execute!(out, crossterm::cursor::Show);
    }
    Ok(())
}

/// Owns the terminal; restores on drop.
pub struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    active: bool,
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode().map_err(|err| {
            io::Error::new(
                err.kind(),
                format!(
                    "{err}; TUI needs a real TTY. Use `cargo run --release -- suggest --history slate:xxxxx` for headless."
                ),
            )
        })?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen).map_err(|err| {
            io::Error::new(
                err.kind(),
                format!(
                    "{err}; TUI needs a real TTY. Use `cargo run --release -- suggest --history slate:xxxxx` for headless."
                ),
            )
        })?;
        NEEDS_RESTORE.store(true, Ordering::SeqCst);
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        Ok(Self {
            terminal,
            active: true,
        })
    }

    pub fn terminal(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }

    pub fn leave(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        NEEDS_RESTORE.store(false, Ordering::SeqCst);
        disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_hook_install_is_idempotent() {
        install_panic_hook();
        install_panic_hook();
        assert!(HOOK_INSTALLED.load(Ordering::SeqCst));
    }
}
