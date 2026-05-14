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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueState {
    pub items: Vec<QueueItem>,
    pub current_index: Option<usize>,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
    pub pending_navigation_index: Option<usize>,
    pub prepared_next_item: Option<QueueItem>,
}

impl QueueState {
    pub fn from_queue(queue: &mut PlayQueue) -> Self {
        queue.prepare_next_cycle_if_needed();
        Self {
            items: queue.items().to_vec(),
            current_index: queue.current_index(),
            shuffle: queue.is_shuffle(),
            repeat_mode: queue.repeat_mode(),
            pending_navigation_index: queue.pending_navigation_index(),
            prepared_next_item: queue.prepared_next_item().cloned(),
        }
    }
}

#[derive(Debug)]
pub struct PlayQueue {
    items: Vec<QueueItem>,
    current_index: Option<usize>,
    shuffle: bool,
    repeat_mode: RepeatMode,
    original_order: Vec<QueueItem>,
    pending_navigation_index: Option<usize>,
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

    pub fn add(&mut self, item: QueueItem) {
        self.invalidate_prepared_shuffle_cycle();
        self.original_order.push(item.clone());
        self.items.push(item);
    }

    pub fn add_many(&mut self, items: Vec<QueueItem>) {
        for item in items {
            self.add(item);
        }
    }

    pub fn insert_next(&mut self, item: QueueItem) {
        self.invalidate_prepared_shuffle_cycle();
        let insert_idx = self.current_index.map(|i| i + 1).unwrap_or(0);
        self.original_order
            .insert(insert_idx.min(self.original_order.len()), item.clone());
        self.items.insert(insert_idx.min(self.items.len()), item);
    }

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

    pub fn remove(&mut self, index: usize) -> Option<QueueItem> {
        if index >= self.items.len() {
            return None;
        }

        self.invalidate_prepared_shuffle_cycle();
        let item = self.items.remove(index);

        if let Some(pos) = self
            .original_order
            .iter()
            .position(|i| i.song_id == item.song_id)
        {
            self.original_order.remove(pos);
        }

        if let Some(current) = self.current_index {
            if index < current {
                self.current_index = Some(current - 1);
                if let Some(pending) = self.pending_navigation_index
                    && index <= pending
                {
                    self.pending_navigation_index = Some(pending.saturating_sub(1));
                }
            } else if index == current {
                self.current_index = None;
                if self.items.is_empty() {
                    self.pending_navigation_index = None;
                } else {
                    self.pending_navigation_index = Some(index.min(self.items.len()));
                }
            }
        } else if let Some(pending) = self.pending_navigation_index {
            if index < pending {
                self.pending_navigation_index = Some(pending - 1);
            } else if index == pending && self.items.is_empty() {
                self.pending_navigation_index = None;
            }
        }

        Some(item)
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.original_order.clear();
        self.current_index = None;
        self.pending_navigation_index = None;
        self.shuffle = false;
        self.repeat_mode = RepeatMode::Off;
        self.prepared_shuffle_cycle = None;
    }

    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return;
        }

        self.invalidate_prepared_shuffle_cycle();
        let item = self.items.remove(from);
        self.items.insert(to, item);
        self.original_order = self.items.clone();

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

    pub fn set_current(&mut self, index: usize) -> Option<&QueueItem> {
        if index < self.items.len() {
            self.invalidate_prepared_shuffle_cycle();
            self.current_index = Some(index);
            self.pending_navigation_index = None;
            self.items.get(index)
        } else {
            None
        }
    }

    pub fn next(&mut self, force: bool) -> Option<&QueueItem> {
        if self.items.is_empty() {
            self.pending_navigation_index = None;
            self.prepared_shuffle_cycle = None;
            return None;
        }

        let effective_index = self.current_index.or(self.pending_navigation_index);

        match self.repeat_mode {
            RepeatMode::One if !force => {
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
                let next_idx = match effective_index {
                    Some(i) if self.current_index.is_some() => (i + 1) % self.items.len(),
                    Some(i) => i.min(self.items.len() - 1),
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
                            None
                        }
                    }
                    Some(i) => Some(i.min(self.items.len() - 1)),
                    None => Some(0),
                };
                self.current_index = next_idx;
                self.pending_navigation_index = None;
                next_idx.and_then(|i| self.items.get(i))
            }
        }
    }

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

    pub fn previous(&mut self) -> Option<&QueueItem> {
        if self.items.is_empty() {
            self.pending_navigation_index = None;
            self.prepared_shuffle_cycle = None;
            return None;
        }

        self.invalidate_prepared_shuffle_cycle();
        let effective_index = self
            .current_index
            .or_else(|| self.pending_navigation_index.map(|i| i.saturating_sub(1)));

        match self.repeat_mode {
            RepeatMode::One => {
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
                    Some(pending.saturating_sub(1))
                } else {
                    match effective_index {
                        Some(i) if i > 0 => Some(i - 1),
                        Some(_) => Some(0),
                        None => Some(0),
                    }
                };
                self.current_index = prev_idx;
                self.pending_navigation_index = None;
                prev_idx.and_then(|i| self.items.get(i))
            }
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.invalidate_prepared_shuffle_cycle();
        self.shuffle = !self.shuffle;

        if self.shuffle {
            let current_item = self.current_item().cloned();
            let mut rng = rand::rng();
            self.items.shuffle(&mut rng);

            if let (Some(current), Some(idx)) = (current_item, self.current_index)
                && let Some(pos) = self.items.iter().position(|i| i.song_id == current.song_id)
            {
                let item = self.items.remove(pos);
                self.items.insert(idx.min(self.items.len()), item);
            }
        } else {
            let current_song_id = self.current_item().map(|i| i.song_id.clone());
            self.items = self.original_order.clone();

            if let Some(song_id) = current_song_id {
                self.current_index = self.items.iter().position(|i| i.song_id == song_id);
            }
        }
    }

    pub fn set_repeat_mode(&mut self, mode: RepeatMode) {
        self.invalidate_prepared_shuffle_cycle();
        self.repeat_mode = mode;
    }

    pub fn cycle_repeat_mode(&mut self) -> RepeatMode {
        self.invalidate_prepared_shuffle_cycle();
        self.repeat_mode = match self.repeat_mode {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        };
        self.repeat_mode
    }

    pub fn peek_next(&mut self) -> Option<&QueueItem> {
        if self.items.is_empty() {
            self.prepared_shuffle_cycle = None;
            return None;
        }

        self.prepare_next_cycle_if_needed();
        let effective_index = self.current_index.or(self.pending_navigation_index);

        match self.repeat_mode {
            RepeatMode::One => self.current_item(),
            RepeatMode::All => {
                if self.should_prepare_wrap_shuffle() {
                    return self.prepared_next_item();
                }

                let next_idx = match effective_index {
                    Some(i) if self.current_index.is_some() => (i + 1) % self.items.len(),
                    Some(i) => i.min(self.items.len() - 1),
                    None => 0,
                };
                self.items.get(next_idx)
            }
            RepeatMode::Off => match effective_index {
                Some(i) if self.current_index.is_some() => {
                    if i + 1 < self.items.len() {
                        self.items.get(i + 1)
                    } else {
                        None
                    }
                }
                Some(i) => self.items.get(i.min(self.items.len() - 1)),
                None => self.items.first(),
            },
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
