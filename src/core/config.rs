//! Shared app/solver configuration (CLI, TUI, and library).

use std::path::PathBuf;

use crate::core::word::Word;
use crate::core::words::OPENING_GUESS;

/// Runtime options shared by CLI and TUI.
#[derive(Clone, Debug)]
pub struct AppConfig {
    /// When true, hard-mode letter constraints are not enforced.
    pub easy_mode: bool,
    /// Opening guess used when the answer list is unrestricted.
    pub opening: Word,
    /// High-contrast / symbolic tile presentation for colorblind users.
    pub colorblind: bool,
    /// Optional override for pattern-cache directory (tests / CI).
    pub cache_dir: Option<PathBuf>,
    /// Optional override for session file path.
    pub session_path: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            easy_mode: false,
            opening: OPENING_GUESS,
            colorblind: false,
            cache_dir: None,
            session_path: None,
        }
    }
}

impl AppConfig {
    pub fn with_easy_mode(mut self, easy: bool) -> Self {
        self.easy_mode = easy;
        self
    }

    pub fn with_opening(mut self, opening: Word) -> Self {
        self.opening = opening;
        self
    }

    pub fn with_colorblind(mut self, on: bool) -> Self {
        self.colorblind = on;
        self
    }

    pub fn with_cache_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.cache_dir = dir;
        self
    }

    pub fn with_session_path(mut self, path: Option<PathBuf>) -> Self {
        self.session_path = path;
        self
    }

    /// Default cache directory: `$WORDLE_SOLVER_CACHE` or `~/.cache/wordle-solver`.
    pub fn resolve_cache_dir(&self) -> PathBuf {
        if let Some(dir) = &self.cache_dir {
            return dir.clone();
        }
        if let Ok(dir) = std::env::var("WORDLE_SOLVER_CACHE") {
            return PathBuf::from(dir);
        }
        default_user_cache_dir()
    }

    /// Default session file: `$WORDLE_SOLVER_SESSION` or `~/.local/share/wordle-solver/session.txt`.
    pub fn resolve_session_path(&self) -> PathBuf {
        if let Some(path) = &self.session_path {
            return path.clone();
        }
        if let Ok(path) = std::env::var("WORDLE_SOLVER_SESSION") {
            return PathBuf::from(path);
        }
        default_user_data_dir().join("session.txt")
    }
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn default_user_cache_dir() -> PathBuf {
    home_dir().join(".cache").join("wordle-solver")
}

pub fn default_user_data_dir() -> PathBuf {
    home_dir()
        .join(".local")
        .join("share")
        .join("wordle-solver")
}

/// Centralized solver tuning knobs (interactive budgets, pool sizes, endgame depths).
#[derive(Clone, Debug)]
pub struct SolverConfig {
    pub interactive_budget_secs: u64,
    pub top_two_ply: usize,
    pub top_two_ply_tight: usize,
    pub full_two_ply_remaining: usize,
    pub early_game_remaining: usize,
    pub early_game_candidates: usize,
    pub early_game_heuristic_prepool: usize,
    pub interactive_two_ply_max: usize,
    pub interactive_early_candidates: usize,
    pub turns_left_remaining_slack: usize,
    pub endgame_probe_max_remaining: usize,
    pub minimax_midgame_max_remaining: usize,
    /// When remaining answers ≤ this, run exact endgame minimax search.
    pub exact_endgame_max_remaining: usize,
    pub tight_turns_partition_cutoff: usize,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            interactive_budget_secs: 10,
            top_two_ply: 55,
            top_two_ply_tight: 75,
            full_two_ply_remaining: 30,
            early_game_remaining: 500,
            early_game_candidates: 1000,
            early_game_heuristic_prepool: 2000,
            interactive_two_ply_max: 110,
            interactive_early_candidates: if cfg!(debug_assertions) { 470 } else { 1000 },
            turns_left_remaining_slack: 2,
            endgame_probe_max_remaining: 16,
            minimax_midgame_max_remaining: 50,
            exact_endgame_max_remaining: 8,
            tight_turns_partition_cutoff: 4,
        }
    }
}

impl SolverConfig {
    pub fn interactive_budget(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.interactive_budget_secs)
    }
}

/// Process-wide default solver knobs (read-only after init).
pub fn solver_config() -> &'static SolverConfig {
    static CFG: std::sync::OnceLock<SolverConfig> = std::sync::OnceLock::new();
    CFG.get_or_init(SolverConfig::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_opening_is_slate() {
        assert_eq!(AppConfig::default().opening.as_str(), "slate");
    }

    #[test]
    fn resolve_cache_dir_prefers_override() {
        let dir = PathBuf::from("/tmp/ws-cache-test");
        let cfg = AppConfig::default().with_cache_dir(Some(dir.clone()));
        assert_eq!(cfg.resolve_cache_dir(), dir);
    }
}
