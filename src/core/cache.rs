//! Versioned on-disk pattern-cache load/save.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::core::patterns::PatternCache;
use crate::core::word::Word;

/// Bump when the binary layout changes.
pub const CACHE_FORMAT_VERSION: u32 = 1;
const MAGIC: &[u8; 8] = b"WLPATC01";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheLoadResult {
    Hit,
    Miss,
    Invalid,
}

/// Build a path for the pattern cache file under `cache_dir`.
pub fn cache_file_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(format!("pattern_cache_v{CACHE_FORMAT_VERSION}.bin"))
}

/// Try to load a pattern cache matching the given word order.
///
/// Returns `Some(cache)` only when the stored answer/guess lists match exactly
/// (same words in the same order). Otherwise returns `None` (caller should rebuild).
pub fn try_load_pattern_cache(
    cache_dir: &Path,
    answers: &[Word],
    guess_pool: &[Word],
) -> Result<(Option<PatternCache>, CacheLoadResult), io::Error> {
    let path = cache_file_path(cache_dir);
    if !path.exists() {
        return Ok((None, CacheLoadResult::Miss));
    }

    let mut file = File::open(&path)?;
    let mut magic = [0u8; 8];
    if file.read_exact(&mut magic).is_err() || &magic != MAGIC {
        return Ok((None, CacheLoadResult::Invalid));
    }

    let version = read_u32(&mut file)?;
    if version != CACHE_FORMAT_VERSION {
        return Ok((None, CacheLoadResult::Invalid));
    }

    let num_answers = read_u32(&mut file)? as usize;
    let num_guesses = read_u32(&mut file)? as usize;
    if num_answers != answers.len() || num_guesses != guess_pool.len() {
        return Ok((None, CacheLoadResult::Invalid));
    }

    let mut stored_answers = vec![0u8; num_answers * 5];
    file.read_exact(&mut stored_answers)?;
    let mut stored_guesses = vec![0u8; num_guesses * 5];
    file.read_exact(&mut stored_guesses)?;

    if !words_match(answers, &stored_answers) || !words_match(guess_pool, &stored_guesses) {
        return Ok((None, CacheLoadResult::Invalid));
    }

    let data_len = num_guesses * num_answers;
    let mut data = vec![0u8; data_len];
    file.read_exact(&mut data)?;

    Ok((
        Some(PatternCache::from_parts(answers, guess_pool, data)),
        CacheLoadResult::Hit,
    ))
}

/// Persist pattern cache for later loads.
pub fn save_pattern_cache(
    cache_dir: &Path,
    answers: &[Word],
    guess_pool: &[Word],
    cache: &PatternCache,
) -> io::Result<()> {
    fs::create_dir_all(cache_dir)?;
    let path = cache_file_path(cache_dir);
    let tmp = cache_dir.join(format!("pattern_cache_v{CACHE_FORMAT_VERSION}.bin.tmp"));

    {
        let mut file = File::create(&tmp)?;
        file.write_all(MAGIC)?;
        write_u32(&mut file, CACHE_FORMAT_VERSION)?;
        write_u32(&mut file, answers.len() as u32)?;
        write_u32(&mut file, guess_pool.len() as u32)?;
        for word in answers {
            file.write_all(&word.0)?;
        }
        for word in guess_pool {
            file.write_all(&word.0)?;
        }
        file.write_all(cache.data_slice())?;
        file.sync_all()?;
    }

    fs::rename(&tmp, &path)?;
    Ok(())
}

fn words_match(words: &[Word], flat: &[u8]) -> bool {
    if flat.len() != words.len() * 5 {
        return false;
    }
    for (i, word) in words.iter().enumerate() {
        if flat[i * 5..(i + 1) * 5] != word.0 {
            return false;
        }
    }
    true
}

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn write_u32(w: &mut impl Write, v: u32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::patterns::PatternCache;
    use crate::core::word::Word;
    use std::time::Instant;

    fn tiny_lists() -> (Vec<Word>, Vec<Word>) {
        let answers = vec![
            Word::parse("crane").unwrap(),
            Word::parse("slate").unwrap(),
            Word::parse("trace").unwrap(),
        ];
        let guesses = vec![
            Word::parse("crane").unwrap(),
            Word::parse("slate").unwrap(),
            Word::parse("trace").unwrap(),
            Word::parse("audio").unwrap(),
        ];
        (answers, guesses)
    }

    #[test]
    fn cache_miss_then_hit_round_trip() {
        let dir =
            std::env::temp_dir().join(format!("wordle-solver-cache-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let (answers, guesses) = tiny_lists();
        let (loaded, status) = try_load_pattern_cache(&dir, &answers, &guesses).unwrap();
        assert!(loaded.is_none());
        assert_eq!(status, CacheLoadResult::Miss);

        let cache = PatternCache::build(&answers, &guesses);
        save_pattern_cache(&dir, &answers, &guesses, &cache).unwrap();

        let (loaded, status) = try_load_pattern_cache(&dir, &answers, &guesses).unwrap();
        assert_eq!(status, CacheLoadResult::Hit);
        let loaded = loaded.expect("cache hit");
        assert_eq!(
            loaded.bucket(Word::parse("slate").unwrap(), Word::parse("crane").unwrap()),
            cache.bucket(Word::parse("slate").unwrap(), Word::parse("crane").unwrap())
        );

        // Second load should still hit (no rebuild path exercised here).
        let start = Instant::now();
        let (loaded2, status2) = try_load_pattern_cache(&dir, &answers, &guesses).unwrap();
        let elapsed = start.elapsed();
        assert_eq!(status2, CacheLoadResult::Hit);
        assert!(loaded2.is_some());
        // Tiny cache: just ensure load completes quickly and succeeds.
        assert!(elapsed.as_secs() < 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_invalid_on_wordlist_change() {
        let dir = std::env::temp_dir().join(format!(
            "wordle-solver-cache-mismatch-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let (answers, guesses) = tiny_lists();
        let cache = PatternCache::build(&answers, &guesses);
        save_pattern_cache(&dir, &answers, &guesses, &cache).unwrap();

        let mut answers2 = answers.clone();
        answers2.push(Word::parse("aback").unwrap());
        let (loaded, status) = try_load_pattern_cache(&dir, &answers2, &guesses).unwrap();
        assert!(loaded.is_none());
        assert_eq!(status, CacheLoadResult::Invalid);

        let _ = fs::remove_dir_all(&dir);
    }
}
