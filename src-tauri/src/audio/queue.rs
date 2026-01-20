use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: i64,
}

#[derive(Debug)]
pub struct PlayQueue {
    items: Vec<QueueItem>,
    current_index: Option<usize>,
    shuffle: bool,
    repeat_mode: RepeatMode,
    // Original order for when shuffle is disabled
    original_order: Vec<QueueItem>,
}

impl Default for PlayQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayQueue {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            current_index: None,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
            original_order: Vec::new(),
        }
    }

    /// Load queue from persisted data
    pub fn load(
        items: Vec<QueueItem>,
        current_index: Option<usize>,
        shuffle: bool,
        repeat_mode: RepeatMode,
    ) -> Self {
        Self {
            original_order: items.clone(),
            items,
            current_index,
            shuffle,
            repeat_mode,
        }
    }

    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    pub fn current_item(&self) -> Option<&QueueItem> {
        self.current_index.and_then(|i| self.items.get(i))
    }

    pub fn is_shuffle(&self) -> bool {
        self.shuffle
    }

    pub fn repeat_mode(&self) -> RepeatMode {
        self.repeat_mode
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Add a single song to the end of the queue
    pub fn add(&mut self, item: QueueItem) {
        self.original_order.push(item.clone());
        self.items.push(item);
    }

    /// Add multiple songs to the end of the queue
    pub fn add_many(&mut self, items: Vec<QueueItem>) {
        for item in items {
            self.add(item);
        }
    }

    /// Insert a song at the next position (play next)
    pub fn insert_next(&mut self, item: QueueItem) {
        let insert_idx = self.current_index.map(|i| i + 1).unwrap_or(0);
        self.original_order
            .insert(insert_idx.min(self.original_order.len()), item.clone());
        self.items.insert(insert_idx.min(self.items.len()), item);
    }

    /// Remove a song at the given index
    pub fn remove(&mut self, index: usize) -> Option<QueueItem> {
        if index >= self.items.len() {
            return None;
        }

        let item = self.items.remove(index);

        // Also remove from original order
        if let Some(pos) = self
            .original_order
            .iter()
            .position(|i| i.song_id == item.song_id)
        {
            self.original_order.remove(pos);
        }

        // Adjust current index
        if let Some(current) = self.current_index {
            if index < current {
                self.current_index = Some(current - 1);
            } else if index == current {
                // Current song was removed - no song in queue is playing now
                // (the audio player still has the old song, but it's not in the queue)
                self.current_index = None;
            }
        }

        Some(item)
    }

    /// Clear the entire queue and reset shuffle state
    pub fn clear(&mut self) {
        self.items.clear();
        self.original_order.clear();
        self.current_index = None;
        self.shuffle = false;
    }

    /// Move an item from one position to another
    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() {
            return;
        }

        let item = self.items.remove(from);
        self.items.insert(to, item);

        // Adjust current index
        if let Some(current) = self.current_index {
            if from == current {
                self.current_index = Some(to);
            } else if from < current && to >= current {
                self.current_index = Some(current - 1);
            } else if from > current && to <= current {
                self.current_index = Some(current + 1);
            }
        }
    }

    /// Set the current playing position
    pub fn set_current(&mut self, index: usize) -> Option<&QueueItem> {
        if index < self.items.len() {
            self.current_index = Some(index);
            self.items.get(index)
        } else {
            None
        }
    }

    /// Get the next song to play
    /// If `force` is true, always advance to next song (ignores RepeatMode::One)
    /// If `force` is false, respect repeat mode (for auto-advance when song ends)
    pub fn next(&mut self, force: bool) -> Option<&QueueItem> {
        if self.items.is_empty() {
            return None;
        }

        match self.repeat_mode {
            RepeatMode::One if !force => {
                // Stay on current song (only when auto-advancing, not user-initiated)
                self.current_item()
            }
            RepeatMode::One | RepeatMode::All => {
                // Wrap around to beginning when reaching end
                let next_idx = match self.current_index {
                    Some(i) => (i + 1) % self.items.len(),
                    None => 0,
                };
                self.current_index = Some(next_idx);
                self.items.get(next_idx)
            }
            RepeatMode::Off => {
                let next_idx = match self.current_index {
                    Some(i) if i + 1 < self.items.len() => Some(i + 1),
                    Some(_) => None, // End of queue
                    None => Some(0),
                };
                self.current_index = next_idx;
                next_idx.and_then(|i| self.items.get(i))
            }
        }
    }

    /// Get the previous song to play
    pub fn previous(&mut self) -> Option<&QueueItem> {
        if self.items.is_empty() {
            return None;
        }

        match self.repeat_mode {
            RepeatMode::One => {
                // Stay on current song
                self.current_item()
            }
            RepeatMode::All => {
                let prev_idx = match self.current_index {
                    Some(0) => self.items.len() - 1,
                    Some(i) => i - 1,
                    None => self.items.len() - 1,
                };
                self.current_index = Some(prev_idx);
                self.items.get(prev_idx)
            }
            RepeatMode::Off => {
                let prev_idx = match self.current_index {
                    Some(i) if i > 0 => Some(i - 1),
                    Some(_) => Some(0), // Stay at beginning
                    None => Some(0),
                };
                self.current_index = prev_idx;
                prev_idx.and_then(|i| self.items.get(i))
            }
        }
    }

    /// Toggle shuffle mode
    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;

        if self.shuffle {
            // Shuffle the queue, keeping current song at its position
            let current_item = self.current_item().cloned();
            let mut rng = rand::rng();
            self.items.shuffle(&mut rng);

            // Move current song back to its original position
            if let (Some(current), Some(idx)) = (current_item, self.current_index) {
                if let Some(pos) = self.items.iter().position(|i| i.song_id == current.song_id) {
                    let item = self.items.remove(pos);
                    self.items.insert(idx.min(self.items.len()), item);
                }
            }
        } else {
            // Restore original order
            let current_song_id = self.current_item().map(|i| i.song_id.clone());
            self.items = self.original_order.clone();

            // Find current song in restored order
            if let Some(song_id) = current_song_id {
                self.current_index = self.items.iter().position(|i| i.song_id == song_id);
            }
        }
    }

    /// Set repeat mode
    pub fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.repeat_mode = mode;
    }

    /// Cycle through repeat modes: Off -> All -> One -> Off
    pub fn cycle_repeat_mode(&mut self) -> RepeatMode {
        self.repeat_mode = match self.repeat_mode {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        self.repeat_mode
    }
}
