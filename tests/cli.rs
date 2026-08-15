use std::process::Command;

use wordle_solver::cli::{parse_args, parse_history};

#[test]
fn parse_history_accepts_slash_and_equals() {
    let slash = parse_history("slate/xxxxx").unwrap();
    let equals = parse_history("slate=xxxxx").unwrap();
    assert_eq!(slash[0].0.as_str(), "slate");
    assert_eq!(equals[0].0.as_str(), "slate");
}

#[test]
fn parse_args_unknown_flag_is_error() {
    let err = parse_args(&["--not-a-flag".into()]).unwrap_err();
    assert!(err.contains("unknown flag"));
}

#[test]
fn suggest_bin_prints_five_letter_word() {
    let output = Command::new(env!("CARGO_BIN_EXE_wordle-solver"))
        .args(["suggest", "--history", "slate:xxxxx"])
        .output()
        .expect("run wordle-solver suggest");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let word = String::from_utf8_lossy(&output.stdout);
    assert_eq!(word.trim().len(), 5);
}

#[test]
fn healthcheck_bin_reports_ok() {
    let output = Command::new(env!("CARGO_BIN_EXE_wordle-solver"))
        .arg("--healthcheck")
        .output()
        .expect("run wordle-solver --healthcheck");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ok healthcheck"));
    assert!(stdout.contains("answers="));
}
