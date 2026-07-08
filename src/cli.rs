//! Headless CLI: suggest next guess without starting the TUI.

use std::process::ExitCode;
use std::sync::Arc;

use wordle_solver::core::config::AppConfig;
use wordle_solver::core::filter::filter_by_history;
use wordle_solver::core::hard_mode::satisfies_hard_mode;
use wordle_solver::core::pattern::Pattern;
use wordle_solver::core::solver::suggest_guess_with_options;
use wordle_solver::core::word::Word;
use wordle_solver::core::words::{load_word_lists, shared_word_lists, OPENING_GUESS};

#[derive(Debug, Default)]
pub struct CliArgs {
    pub suggest: bool,
    pub history: Vec<(Word, Pattern)>,
    pub easy: bool,
    pub opening: Option<Word>,
    pub colorblind: bool,
    pub help: bool,
    pub version: bool,
    /// Launch TUI even if other flags present (default when no subcommand).
    pub tui: bool,
}

pub fn parse_args(args: &[String]) -> Result<CliArgs, String> {
    let mut out = CliArgs {
        tui: true,
        ..CliArgs::default()
    };
    let mut i = 0usize;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "suggest" => {
                out.suggest = true;
                out.tui = false;
            }
            "--help" | "-h" | "help" => out.help = true,
            "--version" | "-V" => out.version = true,
            "--easy" => out.easy = true,
            "--hard" => out.easy = false,
            "--colorblind" => out.colorblind = true,
            "--opener" | "--opening" => {
                i += 1;
                let w = args
                    .get(i)
                    .ok_or_else(|| "--opener requires a 5-letter word".to_string())?;
                out.opening = Some(Word::parse(w).ok_or_else(|| format!("invalid opener: {w}"))?);
            }
            "--history" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or_else(|| "--history requires value".to_string())?;
                out.history = parse_history(raw)?;
            }
            "--tui" => out.tui = true,
            other if other.starts_with('-') => {
                return Err(format!("unknown flag: {other}"));
            }
            other => return Err(format!("unknown argument: {other}")),
        }
        i += 1;
    }
    Ok(out)
}

/// Parse `word:pattern,word:pattern` or `word/pattern;...`.
pub fn parse_history(raw: &str) -> Result<Vec<(Word, Pattern)>, String> {
    let mut history = Vec::new();
    if raw.trim().is_empty() {
        return Ok(history);
    }
    for part in raw.split([',', ';']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (word_s, pat_s) = part.split_once([':', '/', '=']).ok_or_else(|| {
            format!("history entry '{part}' must be word:pattern (e.g. slate:xxxxx)")
        })?;
        let word =
            Word::parse(word_s).ok_or_else(|| format!("invalid guess in history: {word_s}"))?;
        let pattern = Pattern::from_str(pat_s)
            .ok_or_else(|| format!("invalid pattern in history: {pat_s}"))?;
        history.push((word, pattern));
    }
    Ok(history)
}

pub fn print_help() {
    println!(
        "\
wordle-solver — NYT Wordle solver (TUI + headless)

Usage:
  wordle-solver                         Launch interactive TUI
  wordle-solver suggest [options]       Print next-guess suggestion
  wordle-solver --help

Suggest options:
  --history slate:xxxxx,crane:xxYxx   Prior turns (guess:G/Y/X pattern)
  --easy                              Disable hard-mode constraints
  --hard                              Enforce hard mode (default)
  --opener WORD                       Opening guess when history is empty
  --colorblind                        (TUI) high-contrast tile symbols

Examples:
  wordle-solver suggest --history slate:xxxxx
  wordle-solver suggest --history slate:Gxxxx --easy
  wordle-solver suggest --opener crane
"
    );
}

pub fn run_suggest(args: &CliArgs) -> ExitCode {
    let opening = args.opening.unwrap_or(OPENING_GUESS);
    let config = AppConfig::default()
        .with_easy_mode(args.easy)
        .with_opening(opening)
        .with_colorblind(args.colorblind);

    // Prefer shared lists when using default cache; still works offline.
    let lists = if config.cache_dir.is_none() {
        shared_word_lists()
    } else {
        Arc::new(load_word_lists(&config))
    };

    let history = &args.history;
    let remaining = filter_by_history(&lists.answers, history);
    let turns_left = 6usize.saturating_sub(history.len());

    if remaining.is_empty() {
        eprintln!("error: no answers remain for this history");
        return ExitCode::from(2);
    }

    if turns_left == 0 {
        eprintln!("error: already used 6 guesses");
        return ExitCode::from(2);
    }

    let suggestion = match suggest_guess_with_options(
        &lists,
        &remaining,
        history,
        Some(turns_left),
        false,
        args.easy,
        opening,
    ) {
        Some(s) => s,
        None => {
            eprintln!("error: no suggestion available");
            return ExitCode::from(3);
        }
    };

    if !args.easy && !satisfies_hard_mode(suggestion.word, history) {
        eprintln!(
            "error: internal: suggested {} violates hard mode",
            suggestion.word
        );
        return ExitCode::from(4);
    }

    println!("{}", suggestion.word);
    eprintln!(
        "remaining={} entropy={:.3} expected_remaining={:.2} easy={} opener={}",
        remaining.len(),
        suggestion.entropy,
        suggestion.expected_remaining,
        args.easy,
        opening
    );
    ExitCode::SUCCESS
}

pub fn config_from_cli(args: &CliArgs) -> AppConfig {
    AppConfig::default()
        .with_easy_mode(args.easy)
        .with_opening(args.opening.unwrap_or(OPENING_GUESS))
        .with_colorblind(args.colorblind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_history_slate_miss() {
        let h = parse_history("slate:xxxxx").unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].0.as_str(), "slate");
    }

    #[test]
    fn parse_args_suggest_easy() {
        let args = vec![
            "suggest".into(),
            "--history".into(),
            "slate:xxxxx".into(),
            "--easy".into(),
            "--opener".into(),
            "crane".into(),
        ];
        let parsed = parse_args(&args).unwrap();
        assert!(parsed.suggest);
        assert!(parsed.easy);
        assert_eq!(parsed.opening.unwrap().as_str(), "crane");
        assert_eq!(parsed.history.len(), 1);
    }
}
