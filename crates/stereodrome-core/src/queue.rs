use rand::RngExt;
use rand::prelude::IndexedRandom;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: i64,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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

/// Where forward navigation resumes after the playing item was removed from the queue.
///
/// Anchored by song id rather than by position: reordering the queue must not silently
/// point the cursor at a different track.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingNavigation {
    At(String),
    /// The removed item was last, so forward navigation has run off the end.
    PastEnd,
}

#[derive(Debug, Clone)]
pub struct PlayQueue {
    items: Vec<QueueItem>,
    current_index: Option<usize>,
    shuffle: bool,
    repeat_mode: RepeatMode,
    original_order: Vec<QueueItem>,
    pending_navigation: Option<PendingNavigation>,
    prepared_shuffle_cycle: Option<Vec<QueueItem>>,
}

impl Default for PlayQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayQueue {
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            current_index: None,
            shuffle: false,
            repeat_mode: RepeatMode::Off,
            original_order: Vec::new(),
            pending_navigation: None,
            prepared_shuffle_cycle: None,
        }
    }

    #[must_use]
    pub fn load(
        items: Vec<QueueItem>,
        current_index: Option<usize>,
        shuffle: bool,
        repeat_mode: RepeatMode,
    ) -> Self {
        Self::load_with_original_order(items.clone(), items, current_index, shuffle, repeat_mode)
    }

    #[must_use]
    pub fn load_with_original_order(
        items: Vec<QueueItem>,
        original_order: Vec<QueueItem>,
        current_index: Option<usize>,
        shuffle: bool,
        repeat_mode: RepeatMode,
    ) -> Self {
        let original_order = if original_order.len() == items.len() {
            original_order
        } else {
            items.clone()
        };

        Self {
            original_order,
            items,
            current_index,
            shuffle,
            repeat_mode,
            pending_navigation: None,
            prepared_shuffle_cycle: None,
        }
    }

    #[must_use]
    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }

    #[must_use]
    pub fn original_order(&self) -> &[QueueItem] {
        &self.original_order
    }

    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    #[must_use]
    pub fn current_item(&self) -> Option<&QueueItem> {
        self.current_index.and_then(|i| self.items.get(i))
    }

    #[must_use]
    pub fn is_shuffle(&self) -> bool {
        self.shuffle
    }

    #[must_use]
    pub fn repeat_mode(&self) -> RepeatMode {
        self.repeat_mode
    }

    /// Position the navigation cursor sits at, or `items.len()` when it is past the last item.
    #[must_use]
    pub fn pending_navigation_index(&self) -> Option<usize> {
        match self.pending_navigation.as_ref()? {
            PendingNavigation::At(song_id) => {
                self.items.iter().position(|item| &item.song_id == song_id)
            }
            PendingNavigation::PastEnd => Some(self.items.len()),
        }
    }

    /// Index forward navigation resumes at, or `None` when the cursor is past the last item.
    fn pending_resume_index(&self) -> Option<usize> {
        match self.pending_navigation.as_ref()? {
            PendingNavigation::At(song_id) => {
                self.items.iter().position(|item| &item.song_id == song_id)
            }
            PendingNavigation::PastEnd => None,
        }
    }

    fn pending_is_past_end(&self) -> bool {
        self.pending_navigation == Some(PendingNavigation::PastEnd)
    }

    /// Anchor the cursor at whichever item now occupies `index`.
    fn pending_navigation_at_slot(&self, index: usize) -> PendingNavigation {
        self.items
            .get(index)
            .map_or(PendingNavigation::PastEnd, |item| {
                PendingNavigation::At(item.song_id.clone())
            })
    }

    #[must_use]
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
        let insert_idx = self.current_index.map_or(0, |i| i.saturating_add(1));
        self.original_order
            .insert(insert_idx.min(self.original_order.len()), item.clone());
        self.items.insert(insert_idx.min(self.items.len()), item);
    }

    pub fn insert_many_next(&mut self, items: Vec<QueueItem>) {
        self.invalidate_prepared_shuffle_cycle();
        let base_idx = self.current_index.map_or(0, |i| i.saturating_add(1));
        for (offset, item) in items.into_iter().enumerate() {
            let idx = base_idx.saturating_add(offset);
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

        if self.items.is_empty() {
            self.current_index = None;
            self.pending_navigation = None;
            return Some(item);
        }

        if let Some(current) = self.current_index {
            if index < current {
                self.current_index = Some(current.saturating_sub(1));
            } else if index == current {
                // The playing item is gone; resume at whatever took its slot.
                self.current_index = None;
                self.pending_navigation = Some(self.pending_navigation_at_slot(index));
            }
        } else if self.pending_navigation == Some(PendingNavigation::At(item.song_id.clone())) {
            // The anchor itself was removed, so the cursor slides onto its successor.
            self.pending_navigation = Some(self.pending_navigation_at_slot(index));
        }

        Some(item)
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.original_order.clear();
        self.current_index = None;
        self.pending_navigation = None;
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
                self.current_index = Some(current.saturating_sub(1));
            } else if from > current && to <= current {
                self.current_index = Some(current.saturating_add(1));
            }
        }
    }

    pub fn set_current(&mut self, index: usize) -> Option<&QueueItem> {
        if index < self.items.len() {
            self.invalidate_prepared_shuffle_cycle();
            self.current_index = Some(index);
            self.pending_navigation = None;
            self.items.get(index)
        } else {
            None
        }
    }

    pub fn next(&mut self, force: bool) -> Option<&QueueItem> {
        if self.items.is_empty() {
            self.pending_navigation = None;
            self.prepared_shuffle_cycle = None;
            return None;
        }

        // The playing item was removed and nothing followed it, so there is no slot to
        // resume at: end playback unless a repeat mode asks to wrap.
        if self.current_index.is_none() && self.pending_is_past_end() {
            self.pending_navigation = None;
            let wraps = match self.repeat_mode {
                RepeatMode::All => true,
                RepeatMode::One => force,
                RepeatMode::Off => false,
            };
            if !wraps {
                return None;
            }
            self.invalidate_prepared_shuffle_cycle();
            self.current_index = Some(0);
            return self.items.first();
        }

        let effective_index = self.current_index.or_else(|| self.pending_resume_index());

        match self.repeat_mode {
            RepeatMode::One if !force => {
                if self.current_index.is_none()
                    && let Some(idx) = self.pending_resume_index()
                {
                    self.pending_navigation = None;
                    self.current_index = Some(idx);
                    return self.items.get(idx);
                }
                self.current_item()
            }
            RepeatMode::One => {
                self.invalidate_prepared_shuffle_cycle();
                let next_idx = match effective_index {
                    Some(i) if self.current_index.is_some() => i
                        .saturating_add(1)
                        .checked_rem(self.items.len())
                        .unwrap_or_default(),
                    Some(i) => i,
                    None => 0,
                };
                self.current_index = Some(next_idx);
                self.pending_navigation = None;
                self.items.get(next_idx)
            }
            RepeatMode::All => {
                if self.should_prepare_wrap_shuffle() {
                    self.prepare_next_cycle_if_needed();

                    if let Some(next_cycle) = self.prepared_shuffle_cycle.take() {
                        self.items = next_cycle;
                        self.current_index = Some(0);
                        self.pending_navigation = None;
                        return self.items.first();
                    }
                }

                let next_idx = match effective_index {
                    Some(i) if self.current_index.is_some() => i
                        .saturating_add(1)
                        .checked_rem(self.items.len())
                        .unwrap_or_default(),
                    Some(i) => i,
                    None => 0,
                };
                self.current_index = Some(next_idx);
                self.pending_navigation = None;
                self.items.get(next_idx)
            }
            RepeatMode::Off => {
                let next_idx = match effective_index {
                    Some(i) if self.current_index.is_some() => {
                        let candidate = i.saturating_add(1);
                        if candidate < self.items.len() {
                            Some(candidate)
                        } else {
                            None
                        }
                    }
                    Some(i) => Some(i),
                    None => Some(0),
                };
                self.current_index = next_idx;
                self.pending_navigation = None;
                next_idx.and_then(|i| self.items.get(i))
            }
        }
    }

    #[must_use]
    pub fn preview_next(&self, force: bool) -> Option<QueueItem> {
        let mut preview = self.clone();
        preview.next(force).cloned()
    }

    fn manual_next_index(&self) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }

        let Some(current) = self.current_index else {
            return match self.pending_navigation.as_ref() {
                Some(PendingNavigation::At(_)) => self.pending_resume_index(),
                Some(PendingNavigation::PastEnd) => match self.repeat_mode {
                    RepeatMode::One | RepeatMode::All => Some(0),
                    RepeatMode::Off => None,
                },
                None => Some(0),
            };
        };

        match self.repeat_mode {
            RepeatMode::One | RepeatMode::All => Some(
                current
                    .saturating_add(1)
                    .checked_rem(self.items.len())
                    .unwrap_or_default(),
            ),
            RepeatMode::Off => {
                let candidate = current.saturating_add(1);
                (candidate < self.items.len()).then_some(candidate)
            }
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
                .filter(|idx| {
                    next_cycle
                        .get(*idx)
                        .is_some_and(|item| item.song_id != current_song_id)
                })
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
            self.pending_navigation = None;
            self.prepared_shuffle_cycle = None;
            return None;
        }

        self.invalidate_prepared_shuffle_cycle();
        // A cursor past the last item resolves to `items.len()`, so stepping back from it
        // lands on the final track.
        let pending_index = self.pending_navigation_index();
        let effective_index = self
            .current_index
            .or_else(|| pending_index.map(|i| i.saturating_sub(1)));

        match self.repeat_mode {
            RepeatMode::One => {
                if self.current_index.is_none()
                    && let Some(pending) = pending_index
                {
                    self.pending_navigation = None;
                    let idx = pending
                        .saturating_sub(1)
                        .min(self.items.len().saturating_sub(1));
                    self.current_index = Some(idx);
                    return self.items.get(idx);
                }
                self.current_item()
            }
            RepeatMode::All => {
                let prev_idx = if let (None, Some(pending)) = (self.current_index, pending_index) {
                    if pending == 0 {
                        self.items.len().saturating_sub(1)
                    } else {
                        pending
                            .saturating_sub(1)
                            .min(self.items.len().saturating_sub(1))
                    }
                } else {
                    match effective_index {
                        Some(0) | None => self.items.len().saturating_sub(1),
                        Some(i) => i.saturating_sub(1),
                    }
                };
                self.current_index = Some(prev_idx);
                self.pending_navigation = None;
                self.items.get(prev_idx)
            }
            RepeatMode::Off => {
                let prev_idx = if let (None, Some(pending)) = (self.current_index, pending_index) {
                    Some(
                        pending
                            .saturating_sub(1)
                            .min(self.items.len().saturating_sub(1)),
                    )
                } else {
                    match effective_index {
                        Some(i) if i > 0 => Some(i.saturating_sub(1)),
                        Some(_) | None => Some(0),
                    }
                };
                self.current_index = prev_idx;
                self.pending_navigation = None;
                prev_idx.and_then(|i| self.items.get(i))
            }
        }
    }

    #[must_use]
    pub fn preview_previous(&self) -> Option<QueueItem> {
        let mut preview = self.clone();
        preview.previous().cloned()
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

        if self.current_index.is_none() && self.pending_is_past_end() {
            return match self.repeat_mode {
                RepeatMode::All => self.items.first(),
                RepeatMode::One | RepeatMode::Off => None,
            };
        }

        let effective_index = self.current_index.or_else(|| self.pending_resume_index());

        match self.repeat_mode {
            RepeatMode::One => self.current_item(),
            RepeatMode::All => {
                if self.should_prepare_wrap_shuffle() {
                    return self.prepared_next_item();
                }

                let next_idx = match effective_index {
                    Some(i) if self.current_index.is_some() => i
                        .saturating_add(1)
                        .checked_rem(self.items.len())
                        .unwrap_or_default(),
                    Some(i) => i,
                    None => 0,
                };
                self.items.get(next_idx)
            }
            RepeatMode::Off => match effective_index {
                Some(i) if self.current_index.is_some() => {
                    let candidate = i.saturating_add(1);
                    if candidate < self.items.len() {
                        self.items.get(candidate)
                    } else {
                        None
                    }
                }
                Some(i) => self.items.get(i),
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
                    self.pending_resume_index()
                        .and_then(|index| self.items.get(index))
                })
                .cloned()
                .into_iter()
                .collect();
        }

        // `items.len()` for a past-end cursor leaves nothing before the wrap, which the
        // repeat-off and repeat-all arms below both handle.
        let start_index = match (self.current_index, self.pending_navigation_index()) {
            (Some(index), _) => index.saturating_add(1),
            (None, Some(index)) => index,
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
            RepeatMode::All
                if self.shuffle && start_index.saturating_add(max_items) > self.items.len() =>
            {
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
                    upcoming.extend(
                        next_cycle
                            .iter()
                            .take(max_items.saturating_sub(before_wrap))
                            .cloned(),
                    );
                }
                upcoming
            }
            RepeatMode::All => (0..max_items)
                .filter_map(|offset| {
                    let index = start_index
                        .saturating_add(offset)
                        .checked_rem(self.items.len())
                        .unwrap_or_default();
                    self.items.get(index).cloned()
                })
                .collect(),
            RepeatMode::One => Vec::new(),
        }
    }

    pub fn prepare_next_cycle_if_needed(&mut self) {
        if !self.shuffle
            || self.repeat_mode != RepeatMode::All
            || self.items.len() <= 1
            || self.pending_navigation.is_some()
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
            && self.pending_navigation.is_none()
            && self.current_index == Some(self.items.len().saturating_sub(1))
    }
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test setup and assertions intentionally fail fast"
)]
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
    fn navigation_previews_match_commits_without_advancing_queue() {
        let mut queue = PlayQueue::load(
            vec![queue_item("a"), queue_item("b"), queue_item("c")],
            Some(1),
            false,
            RepeatMode::Off,
        );

        let next_preview = queue.preview_next(true).unwrap();
        assert_eq!(next_preview.song_id, "c");
        assert_eq!(queue.current_index(), Some(1));
        assert_eq!(queue.next(true).unwrap().song_id, next_preview.song_id);

        let previous_preview = queue.preview_previous().unwrap();
        assert_eq!(previous_preview.song_id, "b");
        assert_eq!(queue.current_index(), Some(2));
        assert_eq!(queue.previous().unwrap().song_id, previous_preview.song_id);
    }

    #[test]
    fn forced_next_preview_respects_repeat_one_manual_navigation() {
        let mut queue = PlayQueue::load(
            vec![queue_item("a"), queue_item("b")],
            Some(0),
            false,
            RepeatMode::One,
        );

        let preview = queue.preview_next(true).unwrap();
        assert_eq!(preview.song_id, "b");
        assert_eq!(queue.current_index(), Some(0));
        assert_eq!(queue.next(true).unwrap().song_id, preview.song_id);
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
        // Nothing followed the removed track, so with repeat off nothing is upcoming.
        assert!(pending_last.peek_upcoming(2).is_empty());

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
    fn loaded_shuffle_restores_persisted_original_order() {
        let mut queue = PlayQueue::load_with_original_order(
            vec![queue_item("c"), queue_item("a"), queue_item("b")],
            vec![queue_item("a"), queue_item("b"), queue_item("c")],
            Some(0),
            true,
            RepeatMode::Off,
        );

        queue.toggle_shuffle();

        assert!(!queue.is_shuffle());
        assert_eq!(song_ids(queue.items()), vec!["a", "b", "c"]);
        assert_eq!(queue.current_index(), Some(2));
    }

    #[test]
    fn removing_current_last_item_ends_playback_instead_of_replaying() {
        let items = vec![queue_item("a"), queue_item("b"), queue_item("c")];

        let mut repeat_off = PlayQueue::load(items.clone(), Some(2), false, RepeatMode::Off);
        repeat_off.remove(2);
        assert_eq!(repeat_off.pending_navigation_index(), Some(2));
        assert!(repeat_off.peek_next().is_none());
        assert!(repeat_off.next(false).is_none());
        assert_eq!(repeat_off.current_index(), None);

        // Repeat-all still wraps to the top of the queue.
        let mut repeat_all = PlayQueue::load(items.clone(), Some(2), false, RepeatMode::All);
        repeat_all.remove(2);
        assert_eq!(
            repeat_all.peek_next().map(|item| item.song_id.as_str()),
            Some("a")
        );
        assert_eq!(
            repeat_all.next(false).map(|item| item.song_id.as_str()),
            Some("a")
        );

        // Stepping backwards from the gap lands on the new final track.
        let mut backwards = PlayQueue::load(items, Some(2), false, RepeatMode::Off);
        backwards.remove(2);
        assert_eq!(
            backwards.previous().map(|item| item.song_id.as_str()),
            Some("b")
        );
    }

    #[test]
    fn pending_navigation_survives_reordering_the_queue() {
        // Disabling shuffle rebuilds the visible order from the canonical order.
        let mut shuffled = PlayQueue::load_with_original_order(
            vec![
                queue_item("c"),
                queue_item("a"),
                queue_item("b"),
                queue_item("d"),
            ],
            vec![
                queue_item("a"),
                queue_item("b"),
                queue_item("c"),
                queue_item("d"),
            ],
            Some(1),
            true,
            RepeatMode::Off,
        );
        shuffled.remove(1);
        let expected = shuffled
            .peek_next()
            .map(|item| item.song_id.clone())
            .expect("a track follows the gap");
        assert_eq!(expected, "b");

        shuffled.toggle_shuffle();
        assert_eq!(song_ids(shuffled.items()), vec!["b", "c", "d"]);
        assert_eq!(
            shuffled.next(false).map(|item| item.song_id.as_str()),
            Some(expected.as_str())
        );

        // Moving items around keeps the cursor on the same track.
        let mut moved = PlayQueue::load(
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
        moved.remove(1);
        moved.move_item(0, 2);
        assert_eq!(song_ids(moved.items()), vec!["c", "d", "a"]);
        assert_eq!(
            moved.next(false).map(|item| item.song_id.as_str()),
            Some("c")
        );
    }

    #[test]
    fn removing_the_pending_anchor_slides_the_cursor_to_its_successor() {
        let mut queue = PlayQueue::load(
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

        queue.remove(1);
        assert_eq!(queue.pending_navigation_index(), Some(1));

        // "c" is the anchor; removing it should hand the cursor to "d".
        queue.remove(1);
        assert_eq!(queue.pending_navigation_index(), Some(1));
        assert_eq!(
            queue.next(false).map(|item| item.song_id.as_str()),
            Some("d")
        );

        // Removing an unrelated earlier track must not shift the cursor off its track.
        let mut unrelated = PlayQueue::load(
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
        unrelated.remove(2);
        unrelated.remove(0);
        assert_eq!(
            unrelated.next(false).map(|item| item.song_id.as_str()),
            Some("d")
        );
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
