use crate::core::pattern::{Pattern, Tile};
use crate::core::word::Word;

/// Letters known to be correct at each position from prior turns' green tiles.
pub fn known_green_letters(history: &[(Word, Pattern)]) -> [Option<u8>; 5] {
    let mut required = [None::<u8>; 5];
    for &(prev_guess, pattern) in history {
        for i in 0..5 {
            if pattern.tiles[i] == Tile::Correct {
                required[i] = Some(prev_guess.0[i]);
            }
        }
    }
    required
}

/// Hard-mode feedback draft: greens locked at positions already revealed on prior turns.
pub fn prefill_feedback_tiles(
    hard_mode: bool,
    history: &[(Word, Pattern)],
    guess: Word,
) -> ([Option<Tile>; 5], usize) {
    let mut tiles = [None; 5];
    if !hard_mode {
        return (tiles, 0);
    }

    let known = known_green_letters(history);
    for i in 0..5 {
        if known[i] == Some(guess.0[i]) {
            tiles[i] = Some(Tile::Correct);
        }
    }

    let cursor = (0..5)
        .find(|&i| tiles[i].is_none())
        .unwrap_or(0);
    (tiles, cursor)
}

pub fn editable_slot_count(fixed: &[Option<u8>; 5]) -> usize {
    fixed.iter().filter(|slot| slot.is_none()).count()
}

pub fn assemble_guess(fixed: &[Option<u8>; 5], typed: &str) -> Option<Word> {
    let needed = editable_slot_count(fixed);
    if typed.len() != needed {
        return None;
    }
    let mut bytes = [0u8; 5];
    let mut ti = 0;
    for i in 0..5 {
        bytes[i] = if let Some(letter) = fixed[i] {
            letter
        } else {
            let b = *typed.as_bytes().get(ti)?;
            ti += 1;
            if !b.is_ascii_lowercase() {
                return None;
            }
            b
        };
    }
    Word::new(bytes)
}

/// NYT hard mode: greens stay fixed; each prior guess's yellow/green letters must
/// appear in the guess at least as many times as in that guess (max across turns).
pub fn satisfies_hard_mode(guess: Word, history: &[(Word, Pattern)]) -> bool {
    if history.is_empty() {
        return true;
    }

    let required_green = known_green_letters(history);
    let mut min_letter_counts = [0u8; 26];

    for &(prev_guess, pattern) in history {
        let mut turn_letter_counts = [0u8; 26];
        for i in 0..5 {
            match pattern.tiles[i] {
                Tile::Correct => {
                    let letter = prev_guess.0[i];
                    if let Some(existing) = required_green[i] {
                        if existing != letter {
                            return false;
                        }
                    }
                    turn_letter_counts[(letter - b'a') as usize] += 1;
                }
                Tile::Present => {
                    let letter = prev_guess.0[i];
                    turn_letter_counts[(letter - b'a') as usize] += 1;
                }
                Tile::Absent => {}
            }
        }
        for (idx, &count) in turn_letter_counts.iter().enumerate() {
            min_letter_counts[idx] = min_letter_counts[idx].max(count);
        }
    }

    for i in 0..5 {
        if let Some(letter) = required_green[i] {
            if guess.0[i] != letter {
                return false;
            }
        }
    }

    let mut guess_counts = [0u8; 26];
    for &letter in &guess.0 {
        guess_counts[(letter - b'a') as usize] += 1;
    }

    for (idx, &required) in min_letter_counts.iter().enumerate() {
        if guess_counts[idx] < required {
            return false;
        }
    }

    true
}

pub fn filter_hard_mode_compliant(pool: &[Word], history: &[(Word, Pattern)]) -> Vec<Word> {
    if history.is_empty() {
        return pool.to_vec();
    }
    pool.iter()
        .copied()
        .filter(|&word| satisfies_hard_mode(word, history))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pattern::Pattern;

    fn w(s: &str) -> Word {
        Word::from_str(s).unwrap()
    }

    fn pat(s: &str) -> Pattern {
        Pattern::from_str(s).unwrap()
    }

    #[test]
    fn requires_green_position() {
        let history = vec![(w("crane"), pat("Gxxxx"))];
        assert!(satisfies_hard_mode(w("crane"), &history));
        assert!(satisfies_hard_mode(w("clamp"), &history));
        assert!(!satisfies_hard_mode(w("slate"), &history));
    }

    #[test]
    fn requires_yellow_letters() {
        let history = vec![(w("crane"), pat("xxxYx"))]; // yellow N
        assert!(satisfies_hard_mode(w("snare"), &history));
        assert!(!satisfies_hard_mode(w("slate"), &history));
    }

    #[test]
    fn requires_yellow_letter_count() {
        let history = vec![(w("speed"), pat("xxYYx"))]; // two yellow E tiles
        assert!(satisfies_hard_mode(w("eerie"), &history));
        assert!(!satisfies_hard_mode(w("lapse"), &history)); // only one e
    }

    #[test]
    fn aggregates_constraints_across_turns() {
        let history = vec![
            (w("slate"), pat("Gxxxx")),
            (w("crane"), pat("xGYYx")),
        ];
        assert!(satisfies_hard_mode(w("srank"), &history));
        assert!(!satisfies_hard_mode(w("crane"), &history)); // missing s at position 0
    }

    #[test]
    fn repeated_green_same_letter_does_not_stack() {
        let history = vec![
            (w("audio"), pat("Gxxxx")),
            (w("alter"), pat("GxxGx")),
            (w("apnea"), pat("GxxGx")),
        ];
        assert!(satisfies_hard_mode(w("agree"), &history));
    }

    #[test]
    fn known_green_letters_from_history() {
        let history = vec![(w("audio"), pat("Gxxxx")), (w("alter"), pat("GxxGx"))];
        assert_eq!(known_green_letters(&history)[0], Some(b'a'));
        assert_eq!(known_green_letters(&history)[3], Some(b'e'));
    }

    #[test]
    fn prefill_feedback_tiles_after_audio() {
        let history = vec![(w("audio"), pat("Gxxxx"))];
        let (tiles, cursor) = prefill_feedback_tiles(true, &history, w("alarm"));
        assert_eq!(tiles[0], Some(Tile::Correct));
        assert_eq!(tiles[1], None);
        assert_eq!(cursor, 1);
    }

    #[test]
    fn assemble_guess_with_fixed_greens() {
        let fixed = known_green_letters(&[(w("audio"), pat("Gxxxx"))]);
        assert_eq!(assemble_guess(&fixed, "larm"), Some(w("alarm")));
    }
}
