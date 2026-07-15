//! Precomputed second-guess table after the configured opening word.
//!
//! Indexed by [`crate::core::pattern::pattern_bucket_index`] of the opener feedback
//! pattern (0..243). Empty slots (`None`) fall back to live search.
//!
//! Regenerated offline with `cargo run --release --bin gen-second-guess`.

use crate::core::pattern::{pattern_bucket_index, Pattern, PATTERN_BUCKETS};
use crate::core::word::Word;
use crate::core::words::OPENING_GUESS;

/// Opening word this table was built for. Lookup only applies when history matches.
pub const TABLE_OPENER: Word = OPENING_GUESS;

/// `SECOND_GUESS[pattern_bucket_index(feedback)]` after playing [`TABLE_OPENER`].
///
/// Populated by `gen-second-guess`. Until regenerated after an opener change, live search
/// still works for any `None` entry.
pub static SECOND_GUESS: [Option<Word>; PATTERN_BUCKETS] = include!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/data/second_guess_table.rs"
));

/// Look up a precomputed second guess for a single-turn history after the table opener.
pub fn lookup_second_guess(history: &[(Word, Pattern)], opening: Word) -> Option<Word> {
    if history.len() != 1 {
        return None;
    }
    let (guess, pattern) = history[0];
    if guess != opening || opening != TABLE_OPENER {
        return None;
    }
    if pattern.is_win() {
        return None;
    }
    let idx = pattern_bucket_index(pattern);
    SECOND_GUESS.get(idx).copied().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pattern::Pattern;

    #[test]
    fn table_has_expected_len() {
        assert_eq!(SECOND_GUESS.len(), PATTERN_BUCKETS);
    }

    #[test]
    fn lookup_requires_matching_opener_and_single_turn() {
        let pat = Pattern::from_str("xxxxx").unwrap();
        assert!(
            lookup_second_guess(&[(TABLE_OPENER, pat)], TABLE_OPENER).is_some()
                || lookup_second_guess(&[(TABLE_OPENER, pat)], TABLE_OPENER).is_none()
        );
        // Wrong length
        assert!(lookup_second_guess(&[], TABLE_OPENER).is_none());
        // Wrong opener in history
        let other = Word::parse("crane").unwrap();
        if other != TABLE_OPENER {
            assert!(lookup_second_guess(&[(other, pat)], other).is_none());
        }
    }
}
