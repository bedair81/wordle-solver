//! Persist and restore an in-progress game session.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

use crate::core::config::AppConfig;
use crate::core::game::{GameState, Turn};
use crate::core::pattern::Pattern;
use crate::core::word::Word;

/// Snapshot of play session fields needed to resume later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub easy_mode: bool,
    pub copilot: bool,
    pub colorblind: bool,
    pub opening: Word,
    pub turns: Vec<Turn>,
}

impl SessionSnapshot {
    pub fn from_game(game: &GameState, copilot: bool, colorblind: bool, opening: Word) -> Self {
        Self {
            easy_mode: game.easy_mode(),
            copilot,
            colorblind,
            opening,
            turns: game.turns.clone(),
        }
    }

    /// Serialize to a stable line-oriented text format.
    pub fn encode(&self) -> String {
        let mut out = String::new();
        out.push_str("version=1\n");
        out.push_str(&format!("easy_mode={}\n", self.easy_mode as u8));
        out.push_str(&format!("copilot={}\n", self.copilot as u8));
        out.push_str(&format!("colorblind={}\n", self.colorblind as u8));
        out.push_str(&format!("opening={}\n", self.opening));
        for turn in &self.turns {
            out.push_str(&format!("turn {} {}\n", turn.guess, turn.pattern));
        }
        out
    }

    pub fn decode(text: &str) -> Result<Self, String> {
        let mut easy_mode = false;
        let mut copilot = false;
        let mut colorblind = false;
        let mut opening = Word::parse("slate").ok_or_else(|| "bad default opening".to_string())?;
        let mut turns = Vec::new();
        let mut saw_version = false;

        for (lineno, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("version=") {
                if rest != "1" {
                    return Err(format!("unsupported session version: {rest}"));
                }
                saw_version = true;
                continue;
            }
            if let Some(rest) = line.strip_prefix("easy_mode=") {
                easy_mode = rest == "1" || rest.eq_ignore_ascii_case("true");
                continue;
            }
            if let Some(rest) = line.strip_prefix("copilot=") {
                copilot = rest == "1" || rest.eq_ignore_ascii_case("true");
                continue;
            }
            if let Some(rest) = line.strip_prefix("colorblind=") {
                colorblind = rest == "1" || rest.eq_ignore_ascii_case("true");
                continue;
            }
            if let Some(rest) = line.strip_prefix("opening=") {
                opening = Word::parse(rest)
                    .ok_or_else(|| format!("line {}: bad opening word", lineno + 1))?;
                continue;
            }
            if let Some(rest) = line.strip_prefix("turn ") {
                let mut parts = rest.split_whitespace();
                let guess = parts
                    .next()
                    .and_then(Word::parse)
                    .ok_or_else(|| format!("line {}: bad turn guess", lineno + 1))?;
                let pattern = parts
                    .next()
                    .and_then(Pattern::from_str)
                    .ok_or_else(|| format!("line {}: bad turn pattern", lineno + 1))?;
                turns.push(Turn { guess, pattern });
                continue;
            }
            return Err(format!("line {}: unknown field", lineno + 1));
        }

        if !saw_version {
            return Err("missing version=".into());
        }

        Ok(Self {
            easy_mode,
            copilot,
            colorblind,
            opening,
            turns,
        })
    }
}

pub fn save_session(path: &Path, snapshot: &SessionSnapshot) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = File::create(&tmp)?;
        file.write_all(snapshot.encode().as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(tmp, path)?;
    Ok(())
}

pub fn load_session(path: &Path) -> io::Result<Option<SessionSnapshot>> {
    if !path.exists() {
        return Ok(None);
    }
    let file = File::open(path)?;
    let mut text = String::new();
    for line in BufReader::new(file).lines() {
        text.push_str(&line?);
        text.push('\n');
    }
    match SessionSnapshot::decode(&text) {
        Ok(s) => Ok(Some(s)),
        Err(e) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
    }
}

pub fn clear_session(path: &Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Apply a snapshot onto a fresh `GameState` (turns replayed with hard-mode rules
/// matching the snapshot).
pub fn restore_into_game(game: &mut GameState, snapshot: &SessionSnapshot) -> Result<(), String> {
    game.reset();
    game.set_easy_mode(snapshot.easy_mode);
    game.set_opening(snapshot.opening);
    for turn in &snapshot.turns {
        game.record_turn(turn.guess, turn.pattern)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn config_from_snapshot(base: &AppConfig, snapshot: &SessionSnapshot) -> AppConfig {
    base.clone()
        .with_easy_mode(snapshot.easy_mode)
        .with_opening(snapshot.opening)
        .with_colorblind(snapshot.colorblind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::words::shared_word_lists;
    use std::sync::Arc;

    #[test]
    fn session_round_trip_save_load() {
        let path =
            std::env::temp_dir().join(format!("wordle-session-test-{}.txt", std::process::id()));
        let _ = fs::remove_file(&path);

        let lists = shared_word_lists();
        let mut game = GameState::new(lists);
        game.record_turn(
            Word::parse("slate").unwrap(),
            Pattern::from_str("xxxxx").unwrap(),
        )
        .unwrap();
        game.record_turn(
            Word::parse("crane").unwrap(),
            Pattern::from_str("xxYxx").unwrap(),
        )
        .unwrap();

        let snap = SessionSnapshot::from_game(&game, true, true, Word::parse("slate").unwrap());
        save_session(&path, &snap).unwrap();

        let loaded = load_session(&path).unwrap().expect("session exists");
        assert_eq!(loaded.turns.len(), 2);
        assert!(loaded.copilot);
        assert!(loaded.colorblind);
        assert_eq!(loaded.turns[0].guess.as_str(), "slate");

        let mut restored = GameState::new(Arc::clone(&game.word_lists));
        restore_into_game(&mut restored, &loaded).unwrap();
        assert_eq!(restored.turns.len(), 2);
        assert_eq!(restored.remaining_count(), game.remaining_count());

        clear_session(&path).unwrap();
        assert!(load_session(&path).unwrap().is_none());
    }

    #[test]
    fn encode_decode_preserves_fields() {
        let snap = SessionSnapshot {
            easy_mode: true,
            copilot: false,
            colorblind: true,
            opening: Word::parse("crane").unwrap(),
            turns: vec![Turn {
                guess: Word::parse("crane").unwrap(),
                pattern: Pattern::from_str("Gxxxx").unwrap(),
            }],
        };
        let again = SessionSnapshot::decode(&snap.encode()).unwrap();
        assert_eq!(again, snap);
    }
}
