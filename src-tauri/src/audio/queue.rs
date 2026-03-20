use rand::RngExt;
use rand::prelude::IndexedRandom;
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
    // When current song is removed, this tracks the position for next/prev navigation
    // (the removed song continues playing, but isn't in the queue)
    pending_navigation_index: Option<usize>,
    // Prepared order for the next repeat-all cycle when shuffle wraps at the end
    prepared_shuffle_cycle: Option<Vec<QueueItem>>,
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
            pending_navigation_index: None,
            prepared_shuffle_cycle: None,
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
            pending_navigation_index: None,
            prepared_shuffle_cycle: None,
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

    pub fn pending_navigation_index(&self) -> Option<usize> {
        self.pending_navigation_index
    }

    pub fn prepared_next_item(&self) -> Option<&QueueItem> {
        self.prepared_shuffle_cycle
            .as_ref()
            .and_then(|items| items.first())
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
        self.invalidate_prepared_shuffle_cycle();
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
        self.invalidate_prepared_shuffle_cycle();
        let insert_idx = self.current_index.map(|i| i + 1).unwrap_or(0);
        self.original_order
            .insert(insert_idx.min(self.original_order.len()), item.clone());
        self.items.insert(insert_idx.min(self.items.len()), item);
    }

    /// Insert multiple songs at the next position (play next), preserving order
    pub fn insert_many_next(&mut self, items: Vec<QueueItem>) {
        self.invalidate_prepared_shuffle_cycle();
        let base_idx = self.current_index.map(|i| i + 1).unwrap_or(0);
        for (offset, item) in items.into_iter().enumerate() {
            let idx = base_idx + offset;
            self.original_order
                .insert(idx.min(self.original_order.len()), item.clone());
            self.items.insert(idx.min(self.items.len()), item);
        }
    }

    /// Remove a song at the given index
    pub fn remove(&mut self, index: usize) -> Option<QueueItem> {
        if index >= self.items.len() {
            return None;
        }

        self.invalidate_prepared_shuffle_cycle();
        let item = self.items.remove(index);

        // Also remove from original order
        if let Some(pos) = self
            .original_order
            .iter()
            .position(|i| i.song_id == item.song_id)
        {
            self.original_order.remove(pos);
        }

        // Adjust current index and pending navigation
        if let Some(current) = self.current_index {
            if index < current {
                self.current_index = Some(current - 1);
                // Also adjust pending navigation if set
                if let Some(pending) = self.pending_navigation_index
                    && index <= pending
                {
                    self.pending_navigation_index = Some(pending.saturating_sub(1));
                }
            } else if index == current {
                // Current song was removed - set pending navigation index
                // so next/prev work correctly from this position
                // The song at `index` is now what was at `index + 1`
                self.current_index = None;
                if self.items.is_empty() {
                    self.pending_navigation_index = None;
                } else {
                    // Store the index where we were - next() will play items[index],
                    // previous() will play items[index - 1]
                    self.pending_navigation_index = Some(index.min(self.items.len()));
                }
            }
        } else if let Some(pending) = self.pending_navigation_index {
            // No current but have pending - adjust it
            if index < pending {
                self.pending_navigation_index = Some(pending - 1);
            } else if index == pending && self.items.is_empty() {
                self.pending_navigation_index = None;
            }
        }

        Some(item)
    }

    /// Clear the entire queue and reset shuffle/repeat state
    pub fn clear(&mut self) {
        self.items.clear();
        self.original_order.clear();
        self.current_index = None;
        self.pending_navigation_index = None;
        self.shuffle = false;
        self.repeat_mode = RepeatMode::Off;
        self.prepared_shuffle_cycle = None;
    }

    /// Move an item from one position to another
    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() {
            return;
        }

        self.invalidate_prepared_shuffle_cycle();
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
            self.invalidate_prepared_shuffle_cycle();
            self.current_index = Some(index);
            self.pending_navigation_index = None; // Clear pending when explicitly setting current
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
            self.pending_navigation_index = None;
            self.prepared_shuffle_cycle = None;
            return None;
        }

        // If we have a pending navigation (current was removed), use that position
        let effective_index = self.current_index.or(self.pending_navigation_index);

        match self.repeat_mode {
            RepeatMode::One if !force => {
                // Stay on current song (only when auto-advancing, not user-initiated)
                // But if current was removed, we need to move to the pending position
                if self.current_index.is_none()
                    && let Some(pending) = self.pending_navigation_index.take()
                {
                    let idx = pending.min(self.items.len() - 1);
                    self.current_index = Some(idx);
                    return self.items.get(idx);
                }
                self.current_item()
            }
            RepeatMode::One => {
                self.invalidate_prepared_shuffle_cycle();
                let next_idx = match effective_index {
                    Some(i) if self.current_index.is_some() => (i + 1) % self.items.len(),
                    Some(i) => i.min(self.items.len() - 1),
                    None => 0,
                };
                self.current_index = Some(next_idx);
                self.pending_navigation_index = None;
                self.items.get(next_idx)
            }
            RepeatMode::All => {
                if self.should_prepare_wrap_shuffle() {
                    self.prepare_next_cycle_if_needed();

                    if let Some(next_cycle) = self.prepared_shuffle_cycle.take() {
                        self.items = next_cycle;
                        self.current_index = Some(0);
                        self.pending_navigation_index = None;
                        return self.items.first();
                    }
                }

                self.invalidate_prepared_shuffle_cycle();

                // Wrap around to beginning when reaching end
                let next_idx = match effective_index {
                    Some(i) if self.current_index.is_some() => (i + 1) % self.items.len(),
                    Some(i) => i.min(self.items.len() - 1), // Pending: go to that position
                    None => 0,
                };
                self.current_index = Some(next_idx);
                self.pending_navigation_index = None;
                self.items.get(next_idx)
            }
            RepeatMode::Off => {
                let next_idx = match effective_index {
                    Some(i) if self.current_index.is_some() => {
                        if i + 1 < self.items.len() {
                            Some(i + 1)
                        } else {
                            None // End of queue
                        }
                    }
                    Some(i) => Some(i.min(self.items.len() - 1)), // Pending: go to that position
                    None => Some(0),
                };
                self.current_index = next_idx;
                self.pending_navigation_index = None;
                next_idx.and_then(|i| self.items.get(i))
            }
        }
    }

    /// Resolve the queue index that would be selected by a manual next action.
    /// This mirrors `next(force = true)` without mutating queue state.
    fn manual_next_index(&self) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }

        let effective_index = self.current_index.or(self.pending_navigation_index);

        match self.repeat_mode {
            RepeatMode::One | RepeatMode::All => match effective_index {
                Some(i) if self.current_index.is_some() => Some((i + 1) % self.items.len()),
                Some(i) => Some(i.min(self.items.len() - 1)),
                None => Some(0),
            },
            RepeatMode::Off => match effective_index {
                Some(i) if self.current_index.is_some() => {
                    if i + 1 < self.items.len() {
                        Some(i + 1)
                    } else {
                        None
                    }
                }
                Some(i) => Some(i.min(self.items.len() - 1)),
                None => Some(0),
            },
        }
    }

    /// Swap the upcoming next track with a random eligible queue item.
    /// Excludes current and next positions from random selection.
    pub fn reroll_next(&mut self) -> bool {
        if self.should_prepare_wrap_shuffle() {
            self.prepare_next_cycle_if_needed();

            let Some(current_song_id) = self.current_item().map(|item| item.song_id.clone()) else {
                return false;
            };
            let Some(next_cycle) = self.prepared_shuffle_cycle.as_mut() else {
                return false;
            };

            let candidates: Vec<usize> = (1..next_cycle.len())
                .filter(|idx| next_cycle[*idx].song_id != current_song_id)
                .collect();

            let mut rng = rand::rng();
            let Some(random_idx) = candidates.choose(&mut rng).copied() else {
                return false;
            };

            next_cycle.swap(0, random_idx);
            return true;
        }

        let Some(current_idx) = self.current_index else {
            return false;
        };

        let Some(next_idx) = self.manual_next_index() else {
            return false;
        };

        let candidates: Vec<usize> = (0..self.items.len())
            .filter(|idx| *idx != current_idx && *idx != next_idx)
            .collect();

        let mut rng = rand::rng();
        let Some(random_idx) = candidates.choose(&mut rng).copied() else {
            return false;
        };

        self.items.swap(next_idx, random_idx);
        true
    }

    /// Get the previous song to play
    pub fn previous(&mut self) -> Option<&QueueItem> {
        if self.items.is_empty() {
            self.pending_navigation_index = None;
            self.prepared_shuffle_cycle = None;
            return None;
        }

        self.invalidate_prepared_shuffle_cycle();

        // If we have a pending navigation (current was removed), use position - 1
        let effective_index = self.current_index.or_else(|| {
            self.pending_navigation_index.map(|i| {
                // For previous, we want to go to index - 1, so subtract 1 from pending
                // (unless at 0, handled below per repeat mode)
                i.saturating_sub(1)
            })
        });

        match self.repeat_mode {
            RepeatMode::One => {
                // Stay on current song, but if removed, go to previous position
                if self.current_index.is_none()
                    && let Some(pending) = self.pending_navigation_index.take()
                {
                    let idx = pending.saturating_sub(1).min(self.items.len() - 1);
                    self.current_index = Some(idx);
                    return self.items.get(idx);
                }
                self.current_item()
            }
            RepeatMode::All => {
                let prev_idx = if let (None, Some(pending)) =
                    (self.current_index, self.pending_navigation_index)
                {
                    // Pending navigation: go to index - 1 (with wrap)
                    if pending == 0 {
                        self.items.len() - 1
                    } else {
                        pending - 1
                    }
                } else {
                    match effective_index {
                        Some(0) => self.items.len() - 1,
                        Some(i) => i - 1,
                        None => self.items.len() - 1,
                    }
                };
                self.current_index = Some(prev_idx);
                self.pending_navigation_index = None;
                self.items.get(prev_idx)
            }
            RepeatMode::Off => {
                let prev_idx = if let (None, Some(pending)) =
                    (self.current_index, self.pending_navigation_index)
                {
                    // Pending navigation: go to index - 1
                    Some(pending.saturating_sub(1))
                } else {
                    match effective_index {
                        Some(i) if i > 0 => Some(i - 1),
                        Some(_) => Some(0), // Stay at beginning
                        None => Some(0),
                    }
                };
                self.current_index = prev_idx;
                self.pending_navigation_index = None;
                prev_idx.and_then(|i| self.items.get(i))
            }
        }
    }

    /// Toggle shuffle mode
    pub fn toggle_shuffle(&mut self) {
        self.invalidate_prepared_shuffle_cycle();
        self.shuffle = !self.shuffle;

        if self.shuffle {
            // Shuffle the queue, keeping current song at its position
            let current_item = self.current_item().cloned();
            let mut rng = rand::rng();
            self.items.shuffle(&mut rng);

            // Move current song back to its original position
            if let (Some(current), Some(idx)) = (current_item, self.current_index)
                && let Some(pos) = self.items.iter().position(|i| i.song_id == current.song_id)
            {
                let item = self.items.remove(pos);
                self.items.insert(idx.min(self.items.len()), item);
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
        self.invalidate_prepared_shuffle_cycle();
        self.repeat_mode = mode;
    }

    /// Cycle through repeat modes: Off -> All -> One -> Off
    pub fn cycle_repeat_mode(&mut self) -> RepeatMode {
        self.invalidate_prepared_shuffle_cycle();
        self.repeat_mode = match self.repeat_mode {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        self.repeat_mode
    }

    /// Peek at the next song without advancing the queue position.
    /// Used for prefetching the next track to enable gapless playback.
    pub fn peek_next(&mut self) -> Option<&QueueItem> {
        if self.items.is_empty() {
            self.prepared_shuffle_cycle = None;
            return None;
        }

        self.prepare_next_cycle_if_needed();
        let effective_index = self.current_index.or(self.pending_navigation_index);

        match self.repeat_mode {
            RepeatMode::One => {
                // In repeat one, next song is the same song
                self.current_item()
            }
            RepeatMode::All => {
                if self.should_prepare_wrap_shuffle() {
                    return self.prepared_next_item();
                }

                // Wrap around to beginning when reaching end
                let next_idx = match effective_index {
                    Some(i) if self.current_index.is_some() => (i + 1) % self.items.len(),
                    Some(i) => i.min(self.items.len() - 1),
                    None => 0,
                };
                self.items.get(next_idx)
            }
            RepeatMode::Off => {
                match effective_index {
                    Some(i) if self.current_index.is_some() => {
                        if i + 1 < self.items.len() {
                            self.items.get(i + 1)
                        } else {
                            None // End of queue
                        }
                    }
                    Some(i) => self.items.get(i.min(self.items.len() - 1)),
                    None => self.items.first(),
                }
            }
        }
    }

    pub fn prepare_next_cycle_if_needed(&mut self) {
        if !self.should_prepare_wrap_shuffle() {
            self.prepared_shuffle_cycle = None;
            return;
        }

        if self.prepared_shuffle_cycle.is_some() {
            return;
        }

        let Some(current_song_id) = self.current_item().map(|item| item.song_id.clone()) else {
            return;
        };

        let mut next_cycle = self.items.clone();
        let mut rng = rand::rng();
        next_cycle.shuffle(&mut rng);

        if next_cycle
            .first()
            .is_some_and(|item| item.song_id == current_song_id)
        {
            let swap_idx = rng.random_range(1..next_cycle.len());
            next_cycle.swap(0, swap_idx);
        }

        self.prepared_shuffle_cycle = Some(next_cycle);
    }

    fn invalidate_prepared_shuffle_cycle(&mut self) {
        self.prepared_shuffle_cycle = None;
    }

    fn should_prepare_wrap_shuffle(&self) -> bool {
        self.shuffle
            && self.repeat_mode == RepeatMode::All
            && self.items.len() > 1
            && self.pending_navigation_index.is_none()
            && self.current_index == Some(self.items.len() - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::{PlayQueue, QueueItem, RepeatMode};

    fn queue_item(id: &str) -> QueueItem {
        QueueItem {
            song_id: id.to_string(),
            title: format!("Song {id}"),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration: 180,
        }
    }

    fn song_ids(items: &[QueueItem]) -> Vec<String> {
        items.iter().map(|item| item.song_id.clone()).collect()
    }

    fn shuffled_repeat_all_queue() -> PlayQueue {
        let mut queue = PlayQueue::new();
        queue.add_many(vec![
            queue_item("a"),
            queue_item("b"),
            queue_item("c"),
            queue_item("d"),
        ]);
        queue.shuffle = true;
        queue.repeat_mode = RepeatMode::All;
        queue.current_index = Some(queue.items.len() - 1);
        queue
    }

    #[test]
    fn peek_next_prepares_stable_wrap_preview_for_shuffle_repeat_all() {
        let mut queue = shuffled_repeat_all_queue();
        let current_song_id = queue.current_item().unwrap().song_id.clone();

        let first_peek = queue.peek_next().unwrap().song_id.clone();
        let second_peek = queue.peek_next().unwrap().song_id.clone();

        assert_eq!(first_peek, second_peek);
        assert_ne!(first_peek, current_song_id);
        assert_eq!(
            queue.prepared_next_item().map(|item| item.song_id.as_str()),
            Some(first_peek.as_str())
        );
    }

    #[test]
    fn next_consumes_prepared_wrap_cycle() {
        let mut queue = shuffled_repeat_all_queue();
        let expected_next = queue.peek_next().unwrap().song_id.clone();
        let current_song_id = queue.current_item().unwrap().song_id.clone();

        let next_song_id = queue.next(false).unwrap().song_id.clone();

        assert_eq!(next_song_id, expected_next);
        assert_eq!(queue.current_index(), Some(0));
        assert_eq!(queue.items().first().unwrap().song_id, expected_next);
        assert!(queue.prepared_shuffle_cycle.is_none());
        assert_ne!(queue.items().first().unwrap().song_id, current_song_id);

        let mut ids = song_ids(queue.items());
        ids.sort_unstable();
        assert_eq!(
            ids,
            vec![
                "a".to_string(),
                "b".to_string(),
                "c".to_string(),
                "d".to_string()
            ]
        );
    }

    #[test]
    fn manual_and_auto_wrap_both_use_prepared_preview() {
        let mut manual_queue = shuffled_repeat_all_queue();
        let manual_preview = manual_queue.peek_next().unwrap().song_id.clone();
        assert_eq!(manual_queue.next(true).unwrap().song_id, manual_preview);

        let mut auto_queue = shuffled_repeat_all_queue();
        let auto_preview = auto_queue.peek_next().unwrap().song_id.clone();
        assert_eq!(auto_queue.next(false).unwrap().song_id, auto_preview);
    }

    #[test]
    fn mutating_queue_invalidates_prepared_cycle() {
        let mut queue = shuffled_repeat_all_queue();
        queue.peek_next();
        assert!(queue.prepared_shuffle_cycle.is_some());

        queue.add(queue_item("e"));
        assert!(queue.prepared_shuffle_cycle.is_none());
        assert!(queue.prepared_next_item().is_none());
    }

    #[test]
    fn repeat_one_and_repeat_off_behavior_stays_unchanged() {
        let items = vec![queue_item("a"), queue_item("b"), queue_item("c")];

        let mut repeat_one = PlayQueue::load(items.clone(), Some(1), true, RepeatMode::One);
        let repeat_one_peek = repeat_one.peek_next().unwrap().song_id.clone();
        let repeat_one_next = repeat_one.next(false).unwrap().song_id.clone();
        assert_eq!(repeat_one_peek, "b");
        assert_eq!(repeat_one_next, "b");
        assert!(repeat_one.prepared_shuffle_cycle.is_none());

        let mut repeat_off = PlayQueue::load(items, Some(2), true, RepeatMode::Off);
        assert!(repeat_off.peek_next().is_none());
        assert!(repeat_off.next(false).is_none());
        assert!(repeat_off.prepared_shuffle_cycle.is_none());
    }

    #[test]
    fn disabling_shuffle_restores_original_order_after_wrap() {
        let mut queue = shuffled_repeat_all_queue();
        let original_order = song_ids(&queue.original_order);

        queue.peek_next();
        queue.next(false);
        queue.toggle_shuffle();

        assert!(!queue.is_shuffle());
        assert_eq!(song_ids(queue.items()), original_order);
        assert!(queue.prepared_shuffle_cycle.is_none());
    }
}
