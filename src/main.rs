mod cli;
mod tui;

use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match cli::parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("Try `wordle-solver --help`.");
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

    if parsed.suggest {
        return cli::run_suggest(&parsed);
    }

    let config = cli::config_from_cli(&parsed);
    if let Err(err) = tui::run(config) {
        eprintln!("Error: {err}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
