use std::path::PathBuf;

use wordle_solver::core::config::AppConfig;
use wordle_solver::core::game::Turn;
use wordle_solver::core::pattern::Pattern;
use wordle_solver::core::session::SessionSnapshot;
use wordle_solver::core::word::Word;

#[test]
fn resolve_session_path_uses_override() {
    let path = PathBuf::from("/tmp/ws-session-override.txt");
    let cfg = AppConfig::default().with_session_path(Some(path.clone()));
    assert_eq!(cfg.resolve_session_path(), path);
}

#[test]
fn resolve_cache_dir_uses_override() {
    let dir = PathBuf::from("/tmp/ws-cache-override");
    let cfg = AppConfig::default().with_cache_dir(Some(dir.clone()));
    assert_eq!(cfg.resolve_cache_dir(), dir);
}

#[test]
fn session_decode_rejects_missing_version() {
    let err = SessionSnapshot::decode("easy_mode=1\n").unwrap_err();
    assert!(err.contains("missing version"));
}

#[test]
fn session_round_trip_preserves_opening() {
    let snap = SessionSnapshot {
        easy_mode: false,
        copilot: true,
        colorblind: false,
        opening: Word::parse("crane").unwrap(),
        turns: vec![Turn {
            guess: Word::parse("crane").unwrap(),
            pattern: Pattern::from_str("Gxxxx").unwrap(),
        }],
    };
    let again = SessionSnapshot::decode(&snap.encode()).unwrap();
    assert_eq!(again, snap);
}
