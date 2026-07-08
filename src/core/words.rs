use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use crate::core::cache::{save_pattern_cache, try_load_pattern_cache, CacheLoadResult};
use crate::core::config::AppConfig;
use crate::core::patterns::PatternCache;
use crate::core::solver::Suggestion;
use crate::core::word::Word;

const ANSWERS_RAW: &str = include_str!("../../data/answers.txt");
const ALLOWED_GUESSES_RAW: &str = include_str!("../../data/allowed_guesses.txt");

/// Hardcoded strong opener — avoids a multi-minute opening computation at startup.
pub const OPENING_GUESS: Word = Word(*b"slate");

static SHARED_LISTS: OnceLock<Arc<WordLists>> = OnceLock::new();

/// Process-wide shared word lists (reuses pattern cache across tests and callers).
pub fn shared_word_lists() -> Arc<WordLists> {
    SHARED_LISTS
        .get_or_init(|| Arc::new(WordLists::load_with_config(&AppConfig::default())))
        .clone()
}

/// Load word lists using the given config (cache dir). Does not update the process-wide shared instance.
pub fn load_word_lists(config: &AppConfig) -> WordLists {
    WordLists::load_with_config(config)
}

#[derive(Clone)]
pub struct WordLists {
    pub answers: Vec<Word>,
    pub guess_pool: Vec<Word>,
    pub pattern_cache: PatternCache,
    answer_set: HashSet<Word>,
    guess_set: HashSet<Word>,
    /// How the pattern cache was obtained on this load.
    pub cache_status: CacheLoadResult,
}

impl WordLists {
    pub fn load() -> Self {
        Self::load_with_config(&AppConfig::default())
    }

    pub fn load_with_config(config: &AppConfig) -> Self {
        let answers = parse_words(ANSWERS_RAW);
        let extra_guesses = parse_words(ALLOWED_GUESSES_RAW);

        let answer_set: HashSet<Word> = answers.iter().copied().collect();
        let mut guess_set = answer_set.clone();
        let mut guess_pool = answers.clone();

        for word in extra_guesses {
            if guess_set.insert(word) {
                guess_pool.push(word);
            }
        }

        guess_pool.sort();

        let cache_dir = config.resolve_cache_dir();
        let (pattern_cache, cache_status) =
            load_or_build_pattern_cache(&cache_dir, &answers, &guess_pool);

        Self {
            answers,
            guess_pool,
            pattern_cache,
            answer_set,
            guess_set,
            cache_status,
        }
    }

    /// Load using only an explicit cache directory (tests).
    pub fn load_with_cache_dir(cache_dir: &Path) -> Self {
        let cfg = AppConfig::default().with_cache_dir(Some(cache_dir.to_path_buf()));
        Self::load_with_config(&cfg)
    }

    pub fn is_valid_guess(&self, word: Word) -> bool {
        self.guess_set.contains(&word)
    }

    pub fn is_answer(&self, word: Word) -> bool {
        self.answer_set.contains(&word)
    }

    pub fn opening_suggestion(&self, opening: Word) -> Suggestion {
        Suggestion {
            word: opening,
            entropy: 0.0,
            expected_remaining: self.answers.len() as f64,
        }
    }

    pub fn opening_guess(&self) -> Word {
        OPENING_GUESS
    }
}

fn load_or_build_pattern_cache(
    cache_dir: &Path,
    answers: &[Word],
    guess_pool: &[Word],
) -> (PatternCache, CacheLoadResult) {
    match try_load_pattern_cache(cache_dir, answers, guess_pool) {
        Ok((Some(cache), CacheLoadResult::Hit)) => (cache, CacheLoadResult::Hit),
        Ok((_, status)) => {
            let cache = PatternCache::build(answers, guess_pool);
            let _ = save_pattern_cache(cache_dir, answers, guess_pool, &cache);
            (cache, status)
        }
        Err(_) => {
            let cache = PatternCache::build(answers, guess_pool);
            let _ = save_pattern_cache(cache_dir, answers, guess_pool, &cache);
            (cache, CacheLoadResult::Miss)
        }
    }
}

fn parse_words(raw: &str) -> Vec<Word> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            Word::parse(line)
        })
        .collect()
}

pub fn default_word_lists() -> Arc<WordLists> {
    shared_word_lists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cache::CacheLoadResult;
    use std::time::Instant;

    #[test]
    fn loads_expected_counts() {
        let lists = shared_word_lists();
        assert!(lists.answers.len() >= 2300);
        assert!(lists.guess_pool.len() >= 12000);
    }

    #[test]
    fn pattern_cache_matches_feedback() {
        use crate::core::feedback::compute_feedback;
        use crate::core::solver::score::pattern_bucket_index;

        let lists = shared_word_lists();
        let guess = Word::parse("slate").unwrap();
        let answer = Word::parse("crate").unwrap();
        let expected = pattern_bucket_index(compute_feedback(guess, answer));
        assert_eq!(lists.pattern_cache.bucket(guess, answer), Some(expected));
    }

    #[test]
    fn disk_cache_second_load_is_hit() {
        let dir = std::env::temp_dir().join(format!("wordle-lists-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // First load may miss and write.
        let first = WordLists::load_with_cache_dir(&dir);
        assert!(matches!(
            first.cache_status,
            CacheLoadResult::Miss | CacheLoadResult::Invalid | CacheLoadResult::Hit
        ));

        let start = Instant::now();
        let second = WordLists::load_with_cache_dir(&dir);
        let elapsed = start.elapsed();
        assert_eq!(
            second.cache_status,
            CacheLoadResult::Hit,
            "second load should hit on-disk cache"
        );
        // Full rebuild is hundreds of ms; hit should be well under that.
        assert!(elapsed.as_millis() < 5_000, "cache hit took {:?}", elapsed);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shared_lists_are_same_arc() {
        let a = shared_word_lists();
        let b = shared_word_lists();
        assert!(Arc::ptr_eq(&a, &b));
    }
}
