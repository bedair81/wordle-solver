use crate::core::pattern::{Pattern, Tile};
use crate::core::word::Word;

/// Compute NYT Wordle feedback for `guess` against `answer`.
pub fn compute_feedback(guess: Word, answer: Word) -> Pattern {
    let mut tiles = [Tile::Absent; 5];
    let mut answer_counts = [0u8; 26];

    for i in 0..5 {
        if guess.0[i] == answer.0[i] {
            tiles[i] = Tile::Correct;
        } else {
            let idx = (answer.0[i] - b'a') as usize;
            answer_counts[idx] += 1;
        }
    }

    for i in 0..5 {
        if tiles[i] == Tile::Correct {
            continue;
        }
        let idx = (guess.0[i] - b'a') as usize;
        if answer_counts[idx] > 0 {
            tiles[i] = Tile::Present;
            answer_counts[idx] -= 1;
        }
    }

    Pattern::new(tiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pattern::Tile;

    fn w(s: &str) -> Word {
        Word::from_str(s).unwrap()
    }

    fn pat(s: &str) -> Pattern {
        Pattern::from_str(s).unwrap()
    }

    #[test]
    fn all_correct_is_win() {
        let fb = compute_feedback(w("crane"), w("crane"));
        assert!(fb.is_win());
    }

    #[test]
    fn speed_vs_eerie() {
        // eerie: e at 0,1,4; r at 2; i at 3
        // speed: s x, p x, e y (pos 2), e y (pos 1 used), d x
        let fb = compute_feedback(w("speed"), w("eerie"));
        assert_eq!(fb.tiles[0], Tile::Absent); // s
        assert_eq!(fb.tiles[1], Tile::Absent); // p
        assert_eq!(fb.tiles[2], Tile::Present); // e wrong pos
        assert_eq!(fb.tiles[3], Tile::Present); // e wrong pos
        assert_eq!(fb.tiles[4], Tile::Absent); // d
    }

    #[test]
    fn robot_vs_brood() {
        let fb = compute_feedback(w("robot"), w("brood"));
        assert_eq!(fb.tiles[0], Tile::Present); // r
        assert_eq!(fb.tiles[1], Tile::Present); // o
        assert_eq!(fb.tiles[2], Tile::Present); // b
        assert_eq!(fb.tiles[3], Tile::Correct); // o
        assert_eq!(fb.tiles[4], Tile::Absent); // t
    }

    #[test]
    fn alloy_vs_hello() {
        let fb = compute_feedback(w("alloy"), w("hello"));
        assert_eq!(fb.tiles[0], Tile::Absent);
        assert_eq!(fb.tiles[1], Tile::Present); // l
        assert_eq!(fb.tiles[2], Tile::Correct); // l
        assert_eq!(fb.tiles[3], Tile::Present); // o -> l leftover
        assert_eq!(fb.tiles[4], Tile::Absent);
    }

    #[test]
    fn matches_declared_pattern() {
        let guess = w("slate");
        let answer = w("crate");
        let fb = compute_feedback(guess, answer);
        assert_eq!(fb, pat("xxGGG"));
    }
}
