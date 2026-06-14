use std::fmt;
use std::str::FromStr;

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

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        if s.len() != 5 || !s.is_ascii() {
            return None;
        }
        let mut bytes = [0u8; 5];
        bytes.copy_from_slice(s.as_bytes());
        Self::new(bytes)
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: all bytes are valid ASCII lowercase letters
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
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_str(s).ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_word() {
        let w = Word::from_str("slate").unwrap();
        assert_eq!(w.as_str(), "slate");
    }

    #[test]
    fn rejects_invalid_word() {
        assert!(Word::from_str("slat").is_none());
        assert!(Word::from_str("slate!").is_none());
    }
}
