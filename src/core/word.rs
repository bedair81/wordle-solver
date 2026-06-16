use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidWord;

impl fmt::Display for InvalidWord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid 5-letter lowercase word")
    }
}

impl std::error::Error for InvalidWord {}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Word(pub [u8; 5]);

impl Word {
    pub fn new(bytes: [u8; 5]) -> Option<Self> {
        if bytes.iter().all(|&b| b.is_ascii_lowercase()) {
            Some(Word(bytes))
        } else {
            None
        }
    }

    /// Parse a trimmed lowercase ASCII 5-letter word.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.len() != 5 || !s.is_ascii() {
            return None;
        }
        let mut bytes = [0u8; 5];
        bytes.copy_from_slice(s.as_bytes());
        Self::new(bytes)
    }

    pub fn as_str(&self) -> &str {
        debug_assert!(
            self.0.iter().all(|&b| b.is_ascii_lowercase()),
            "Word invariant violated"
        );
        // SAFETY: all bytes are valid ASCII lowercase letters (enforced by `new` / `parse`).
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }

    pub fn letters(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.iter().copied()
    }

    pub fn unique_letter_count(&self) -> usize {
        let mut seen = [false; 26];
        let mut count = 0;
        for &b in &self.0 {
            let idx = (b - b'a') as usize;
            if !seen[idx] {
                seen[idx] = true;
                count += 1;
            }
        }
        count
    }
}

impl fmt::Display for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl fmt::Debug for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Word({})", self.as_str())
    }
}

impl FromStr for Word {
    type Err = InvalidWord;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(InvalidWord)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_word() {
        let w = Word::parse("slate").unwrap();
        assert_eq!(w.as_str(), "slate");
        assert_eq!(Word::from_str("slate").unwrap(), w);
    }

    #[test]
    fn rejects_invalid_word() {
        assert!(Word::parse("slat").is_none());
        assert!(Word::parse("slate!").is_none());
        assert!(Word::from_str("slat").is_err());
    }
}
