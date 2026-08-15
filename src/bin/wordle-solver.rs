use std::env;
use std::io::{self, IsTerminal};
use std::process::ExitCode;

use wordle_solver::cli;
use wordle_solver::tui;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match cli::parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("Try `cargo run --release -- --help` or `wordle-solver --help`.");
            return ExitCode::from(1);
        }
    };

    if parsed.help {
        cli::print_help();
        return ExitCode::SUCCESS;
    }
    if parsed.version {
        println!("wordle-solver {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if parsed.healthcheck {
        return cli::run_healthcheck();
    }
    if parsed.suggest {
        return cli::run_suggest(&parsed);
    }

    if !io::stdout().is_terminal() {
        eprintln!("error: TUI requires a real TTY.");
        eprintln!("For headless use: cargo run --release -- suggest --history slate:xxxxx");
        eprintln!("Or: ./bin/wordle-solver suggest --history slate:xxxxx");
        return ExitCode::from(1);
    }

    let config = cli::config_from_cli(&parsed);
    if let Err(err) = tui::run(config) {
        eprintln!("Error: {err}");
        eprintln!("TUI requires a real TTY. For headless use:");
        eprintln!("  cargo run --release -- suggest --history slate:xxxxx");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
