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
        self.should_prepare_wrap_shuffle()
            .then(|| self.prepared_shuffle_cycle.as_ref()?.first())
            .flatten()
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

    /// Return upcoming queue items in playback order without advancing the queue.
    /// At most one queue cycle is returned, so repeat modes never produce duplicates
    /// solely to fill `limit`.
    pub fn peek_upcoming(&mut self, limit: usize) -> Vec<QueueItem> {
        if limit == 0 || self.items.is_empty() {
            return Vec::new();
        }

        self.prepare_next_cycle_if_needed();
        let max_items = limit.min(self.items.len());

        if self.repeat_mode == RepeatMode::One {
            return self
                .current_item()
                .or_else(|| {
                    self.pending_navigation_index
                        .and_then(|index| self.items.get(index.min(self.items.len() - 1)))
                })
                .cloned()
                .into_iter()
                .collect();
        }

        let start_index = match (self.current_index, self.pending_navigation_index) {
            (Some(index), _) => index + 1,
            (None, Some(index)) => index.min(self.items.len() - 1),
            (None, None) => 0,
        };

        match self.repeat_mode {
            RepeatMode::Off => self
                .items
                .iter()
                .skip(start_index)
                .take(max_items)
                .cloned()
                .collect(),
            RepeatMode::All if self.shuffle && start_index + max_items > self.items.len() => {
                let before_wrap = self.items.len().saturating_sub(start_index);
                self.prepare_shuffle_cycle_after_wrap();
                let mut upcoming = self
                    .items
                    .iter()
                    .skip(start_index)
                    .take(max_items)
                    .cloned()
                    .collect::<Vec<_>>();
                if let Some(next_cycle) = &self.prepared_shuffle_cycle {
                    upcoming.extend(next_cycle.iter().take(max_items - before_wrap).cloned());
                }
                upcoming
            }
            RepeatMode::All => (0..max_items)
                .map(|offset| self.items[(start_index + offset) % self.items.len()].clone())
                .collect(),
            RepeatMode::One => unreachable!("repeat-one handled above"),
        }
    }

    pub fn prepare_next_cycle_if_needed(&mut self) {
        if !self.shuffle
            || self.repeat_mode != RepeatMode::All
            || self.items.len() <= 1
            || self.pending_navigation_index.is_some()
        {
            self.prepared_shuffle_cycle = None;
            return;
        }

        if self.should_prepare_wrap_shuffle() {
            self.prepare_shuffle_cycle_after_wrap();
        }
    }

    fn prepare_shuffle_cycle_after_wrap(&mut self) {
        if self.prepared_shuffle_cycle.is_some() || self.items.len() <= 1 {
            return;
        }

        let Some(wrapping_song_id) = self.items.last().map(|item| item.song_id.clone()) else {
            return;
        };
        let mut next_cycle = self.items.clone();
        let mut rng = rand::rng();
        next_cycle.shuffle(&mut rng);

        if next_cycle
            .first()
            .is_some_and(|item| item.song_id == wrapping_song_id)
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
    fn move_item_updates_canonical_order() {
        let mut queue = PlayQueue::load(
            vec![
                queue_item("a"),
                queue_item("b"),
                queue_item("c"),
                queue_item("d"),
            ],
            Some(2),
            true,
            RepeatMode::Off,
        );

        queue.items = vec![
            queue_item("c"),
            queue_item("a"),
            queue_item("d"),
            queue_item("b"),
        ];
        queue.original_order = vec![
            queue_item("a"),
            queue_item("b"),
            queue_item("c"),
            queue_item("d"),
        ];

        queue.move_item(0, 2);

        assert_eq!(
            song_ids(queue.items()),
            vec![
                "a".to_string(),
                "d".to_string(),
                "c".to_string(),
                "b".to_string()
            ]
        );
        assert_eq!(song_ids(&queue.original_order), song_ids(queue.items()));
    }

    #[test]
    fn move_item_repositions_current_track() {
        let mut move_current = PlayQueue::load(
            vec![
                queue_item("a"),
                queue_item("b"),
                queue_item("c"),
                queue_item("d"),
            ],
            Some(1),
            false,
            RepeatMode::Off,
        );

        move_current.move_item(1, 3);
        assert_eq!(move_current.current_index(), Some(3));

        let mut move_before_current = PlayQueue::load(
            vec![
                queue_item("a"),
                queue_item("b"),
                queue_item("c"),
                queue_item("d"),
            ],
            Some(2),
            false,
            RepeatMode::Off,
        );

        move_before_current.move_item(0, 3);
        assert_eq!(move_before_current.current_index(), Some(1));

        let mut move_after_current = PlayQueue::load(
            vec![
                queue_item("a"),
                queue_item("b"),
                queue_item("c"),
                queue_item("d"),
            ],
            Some(2),
            false,
            RepeatMode::Off,
        );

        move_after_current.move_item(3, 1);
        assert_eq!(move_after_current.current_index(), Some(3));
    }

    #[test]
    fn move_item_invalidates_prepared_shuffle_cycle() {
        let mut queue = shuffled_repeat_all_queue();
        queue.peek_next();
        assert!(queue.prepared_shuffle_cycle.is_some());

        queue.move_item(0, 2);

        assert!(queue.prepared_shuffle_cycle.is_none());
        assert!(queue.prepared_next_item().is_none());
    }

    #[test]
    fn disabling_shuffle_restores_reordered_visible_order() {
        let mut queue = PlayQueue::load(
            vec![
                queue_item("a"),
                queue_item("b"),
                queue_item("c"),
                queue_item("d"),
            ],
            Some(2),
            true,
            RepeatMode::Off,
        );

        queue.items = vec![
            queue_item("c"),
            queue_item("a"),
            queue_item("d"),
            queue_item("b"),
        ];
        queue.original_order = vec![
            queue_item("a"),
            queue_item("b"),
            queue_item("c"),
            queue_item("d"),
        ];

        queue.move_item(0, 2);
        let reordered = song_ids(queue.items());
        let current_song_id = queue.current_item().unwrap().song_id.clone();

        queue.toggle_shuffle();

        assert!(!queue.is_shuffle());
        assert_eq!(song_ids(queue.items()), reordered);
        assert_eq!(
            queue.current_item().map(|item| item.song_id.as_str()),
            Some(current_song_id.as_str())
        );
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
    fn peek_upcoming_returns_multiple_items_without_advancing() {
        let mut queue = PlayQueue::load(
            vec![
                queue_item("a"),
                queue_item("b"),
                queue_item("c"),
                queue_item("d"),
            ],
            Some(0),
            false,
            RepeatMode::Off,
        );

        assert_eq!(song_ids(&queue.peek_upcoming(3)), vec!["b", "c", "d"]);
        assert_eq!(queue.current_index(), Some(0));
    }

    #[test]
    fn peek_upcoming_wraps_once_for_repeat_all() {
        let mut queue = PlayQueue::load(
            vec![queue_item("a"), queue_item("b"), queue_item("c")],
            Some(1),
            false,
            RepeatMode::All,
        );

        assert_eq!(song_ids(&queue.peek_upcoming(10)), vec!["c", "a", "b"]);
    }

    #[test]
    fn peek_upcoming_uses_stable_shuffled_wrap_cycle() {
        let mut queue = shuffled_repeat_all_queue();
        let preview = queue.peek_upcoming(3);

        assert_eq!(preview.len(), 3);
        assert_eq!(
            preview.first().map(|item| item.song_id.as_str()),
            queue.peek_next().map(|item| item.song_id.as_str())
        );
        assert_eq!(song_ids(&preview), song_ids(&queue.peek_upcoming(3)));
    }

    #[test]
    fn shuffled_lookahead_persists_the_prepared_wrap_cycle() {
        let mut queue = PlayQueue::load(
            vec![
                queue_item("a"),
                queue_item("b"),
                queue_item("c"),
                queue_item("d"),
            ],
            Some(1),
            true,
            RepeatMode::All,
        );

        let upcoming = queue.peek_upcoming(4);
        assert_eq!(song_ids(&upcoming[..2]), vec!["c", "d"]);
        let expected_after_wrap = upcoming[2].song_id.clone();
        assert!(queue.prepared_next_item().is_none());

        queue.next(false);
        queue.next(false);
        assert_eq!(
            queue.peek_next().map(|item| item.song_id.as_str()),
            Some(expected_after_wrap.as_str())
        );
    }

    #[test]
    fn peek_upcoming_respects_pending_navigation_and_repeat_one() {
        let mut pending = PlayQueue::load(
            vec![queue_item("a"), queue_item("b"), queue_item("c")],
            Some(1),
            false,
            RepeatMode::Off,
        );
        pending.remove(1);
        assert_eq!(song_ids(&pending.peek_upcoming(2)), vec!["c"]);

        let mut pending_last = PlayQueue::load(
            vec![queue_item("a"), queue_item("b"), queue_item("c")],
            Some(2),
            false,
            RepeatMode::Off,
        );
        pending_last.remove(2);
        assert_eq!(song_ids(&pending_last.peek_upcoming(2)), vec!["b"]);

        let mut repeat_one = PlayQueue::load(
            vec![queue_item("a"), queue_item("b"), queue_item("c")],
            Some(1),
            false,
            RepeatMode::One,
        );
        assert_eq!(song_ids(&repeat_one.peek_upcoming(5)), vec!["b"]);
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

    #[test]
    fn removing_current_item_preserves_pending_navigation_position() {
        let mut queue = PlayQueue::load(
            vec![queue_item("a"), queue_item("b"), queue_item("c")],
            Some(1),
            false,
            RepeatMode::Off,
        );

        let removed = queue.remove(1).unwrap();

        assert_eq!(removed.song_id, "b");
        assert_eq!(queue.current_index(), None);
        assert_eq!(queue.pending_navigation_index(), Some(1));
        assert_eq!(
            queue.next(false).map(|item| item.song_id.as_str()),
            Some("c")
        );
        assert_eq!(queue.current_index(), Some(1));
        assert_eq!(queue.pending_navigation_index(), None);
    }
}
