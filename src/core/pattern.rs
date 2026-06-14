use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Tile {
    Correct,
    Present,
    Absent,
}

impl Tile {
    pub fn from_char(c: char) -> Option<Self> {
        match c.to_ascii_lowercase() {
            'g' => Some(Tile::Correct),
            'y' => Some(Tile::Present),
            'x' | 'a' | 'b' => Some(Tile::Absent),
            _ => None,
        }
    }

    pub fn to_char(self) -> char {
        match self {
            Tile::Correct => 'G',
            Tile::Present => 'Y',
            Tile::Absent => 'X',
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            Tile::Absent => Tile::Correct,
            Tile::Correct => Tile::Present,
            Tile::Present => Tile::Absent,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Pattern {
    pub tiles: [Tile; 5],
    key: u32,
}

impl Pattern {
    pub fn new(tiles: [Tile; 5]) -> Self {
        let mut key = 0u32;
        for (i, tile) in tiles.iter().enumerate() {
            let val = match tile {
                Tile::Absent => 0,
                Tile::Present => 1,
                Tile::Correct => 2,
            };
            key |= val << (i * 2);
        }
        Self { tiles, key }
    }

    pub fn key(&self) -> u32 {
        self.key
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.len() != 5 {
            return None;
        }
        let mut tiles = [Tile::Absent; 5];
        for (i, c) in s.chars().enumerate() {
            tiles[i] = Tile::from_char(c)?;
        }
        Some(Self::new(tiles))
    }

    pub fn is_win(&self) -> bool {
        self.tiles.iter().all(|t| matches!(t, Tile::Correct))
    }

    pub fn all_absent() -> Self {
        Self::new([Tile::Absent; 5])
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for tile in &self.tiles {
            write!(f, "{}", tile.to_char())?;
        }
        Ok(())
    }
}
