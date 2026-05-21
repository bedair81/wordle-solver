use std::collections::HashMap;

use crate::core::feedback::compute_feedback;
use crate::core::filter::filter_by_history;
use crate::core::pattern::{Pattern, Tile};
use crate::core::word::Word;
use crate::core::words::WordLists;

pub const OPENING_GUESS: Word = Word(*b"slate");

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub word: Word,
    pub entropy: f64,
    pub expected_remaining: f64,
}

pub fn suggest_guess(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    history: &[(Word, Pattern)],
    hard_mode: bool,
) -> Option<Suggestion> {
    if remaining_answers.is_empty() {
        return None;
    }

    if remaining_answers.len() == 1 {
        let word = remaining_answers[0];
        return Some(Suggestion {
            word,
            entropy: 0.0,
            expected_remaining: 1.0,
        });
    }

    let guess_candidates = select_guess_candidates(word_lists, remaining_answers, hard_mode, history);

    let total = remaining_answers.len() as f64;
    let mut best: Option<Suggestion> = None;

    for &guess in &guess_candidates {
        let mut buckets: HashMap<u32, usize> = HashMap::new();
        for &answer in remaining_answers {
            let pattern = compute_feedback(guess, answer);
            *buckets.entry(pattern.key()).or_insert(0) += 1;
        }

        let entropy = shannon_entropy(&buckets, total);
        let expected = buckets.values().map(|&c| {
            let p = c as f64 / total;
            p * c as f64
        }).sum::<f64>();

        let suggestion = Suggestion {
            word: guess,
            entropy,
            expected_remaining: expected,
        };

        best = Some(match best {
            None => suggestion,
            Some(prev) => {
                if suggestion.entropy > prev.entropy + 1e-9 {
                    suggestion
                } else if (suggestion.entropy - prev.entropy).abs() < 1e-9 {
                    if frequency_score(guess) > frequency_score(prev.word) {
                        suggestion
                    } else if frequency_score(guess) == frequency_score(prev.word) && guess < prev.word {
                        suggestion
                    } else {
                        prev
                    }
                } else {
                    prev
                }
            }
        });
    }

    best
}

fn select_guess_candidates(
    word_lists: &WordLists,
    remaining_answers: &[Word],
    hard_mode: bool,
    history: &[(Word, Pattern)],
) -> Vec<Word> {
    if remaining_answers.len() <= 2 {
        return remaining_answers.to_vec();
    }

    let pool = if hard_mode {
        filter_by_history(&word_lists.guess_pool, history)
    } else {
        word_lists.guess_pool.clone()
    };

    if remaining_answers.len() > 500 {
        let mut scored: Vec<(Word, usize)> = pool
            .iter()
            .map(|&w| (w, w.unique_letter_count() * 10 + frequency_score(w)))
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        scored.into_iter().take(800).map(|(w, _)| w).collect()
    } else {
        pool
    }
}

fn shannon_entropy(buckets: &HashMap<u32, usize>, total: f64) -> f64 {
    buckets.values().fold(0.0, |acc, &count| {
        let p = count as f64 / total;
        if p > 0.0 {
            acc - p * p.log2()
        } else {
            acc
        }
    })
}

fn frequency_score(word: Word) -> usize {
    const FREQ: [usize; 26] = [
        8, 2, 5, 4, 12, 3, 4, 5, 7, 1, 1, 6, 5, 7, 6, 3, 1, 9, 6, 4, 3, 2, 2, 1, 3, 1,
    ];
    word.letters().map(|b| FREQ[(b - b'a') as usize]).sum()
}

pub fn satisfies_hard_mode(guess: Word, history: &[(Word, Pattern)]) -> bool {
    for &(prev_guess, pattern) in history {
        for i in 0..5 {
            match pattern.tiles[i] {
                Tile::Correct => {
                    if guess.0[i] != prev_guess.0[i] {
                        return false;
                    }
                }
                Tile::Present => {
                    if !guess.0.contains(&prev_guess.0[i]) {
                        return false;
                    }
                }
                Tile::Absent => {}
            }
        }
    }
    true
}

pub fn auto_solve(word_lists: &WordLists, target: Word) -> Option<Vec<(Word, Pattern)>> {
    let mut history = Vec::new();
    let max_turns = 6;

    for _ in 0..max_turns {
        let remaining = filter_by_history(&word_lists.answers, &history);
        if remaining.contains(&target) && history.last().map(|(_, p)| p.is_win()).unwrap_or(false) {
            break;
        }

        let suggestion = if history.is_empty() {
            Some(Suggestion {
                word: OPENING_GUESS,
                entropy: 0.0,
                expected_remaining: 0.0,
            })
        } else {
            suggest_guess(word_lists, &remaining, &history, false)
        }?;

        let guess = suggestion.word;
        let pattern = compute_feedback(guess, target);
        history.push((guess, pattern));

        if pattern.is_win() {
            return Some(history);
        }
    }

    if history.last().map(|(_, p)| p.is_win()).unwrap_or(false) {
        Some(history)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::words::WordLists;

    fn w(s: &str) -> Word {
        Word::from_str(s).unwrap()
    }

    #[test]
    fn opening_guess_is_valid() {
        let lists = WordLists::load();
        assert!(lists.is_valid_guess(OPENING_GUESS));
    }

    #[test]
    fn suggests_from_remaining() {
        let lists = WordLists::load();
        let remaining = vec![w("crane"), w("grape")];
        let suggestion = suggest_guess(&lists, &remaining, &[], false).unwrap();
        assert!(remaining.contains(&suggestion.word) || lists.is_valid_guess(suggestion.word));
    }
}
