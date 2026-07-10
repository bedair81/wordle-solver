use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

use crate::core::pattern::Pattern;
use crate::core::word::Word;
use crate::core::words::WordLists;

use super::{suggest_guess_interactive, Suggestion};

/// Async suggestion job: compute off the calling thread; poll with a generation counter.
pub struct SuggestionJob {
    generation: u64,
    rx: Receiver<(u64, Option<Suggestion>)>,
}

impl SuggestionJob {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Non-blocking poll. `None` means still running; `Some` is the finished result
    /// (only returned when `generation` still matches).
    pub fn try_recv(&self) -> Option<Option<Suggestion>> {
        match self.rx.try_recv() {
            Ok((gen, suggestion)) if gen == self.generation => Some(suggestion),
            Ok(_) => Some(None), // stale — treat as no suggestion
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(None),
        }
    }

    /// Test/helper constructor for injecting a pre-built channel.
    #[cfg(test)]
    pub(crate) fn from_parts(generation: u64, rx: Receiver<(u64, Option<Suggestion>)>) -> Self {
        Self { generation, rx }
    }
}

/// Spawn a background suggestion computation. Caller should bump `generation` on undo/reset.
pub fn spawn_suggestion_job(
    word_lists: Arc<WordLists>,
    remaining: Vec<Word>,
    history: Vec<(Word, Pattern)>,
    turns_left: usize,
    easy_mode: bool,
    opening: Word,
    generation: u64,
) -> SuggestionJob {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = suggest_guess_interactive(
            &word_lists,
            &remaining,
            &history,
            turns_left,
            easy_mode,
            opening,
        );
        let _ = tx.send((generation, result));
    });
    SuggestionJob { generation, rx }
}
