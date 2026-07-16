use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use rusqlite::params_from_iter;
use serde::Serialize;

use crate::audio::queue::{PlayQueue, QueueItem, RepeatMode};
use crate::db::queue::save_queue;
use crate::error::{AppError, AppResult, MutexExt};
use crate::operations::settings::read_playback_settings;
use crate::state::DesktopState;

#[derive(Debug, Clone, Serialize)]
pub struct QueueState {
    pub items: Vec<QueueItem>,
    pub current_index: Option<usize>,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
    pub pending_navigation_index: Option<usize>,
    pub prepared_next_item: Option<QueueItem>,
}

impl QueueState {
    fn from_queue(queue: &PlayQueue) -> Self {
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

/// Save queue to database and emit queue-changed event
pub fn persist_and_emit(state: &DesktopState) {
    let mut queue = state.queue.lock_recover();
    queue.prepare_next_cycle_if_needed();
    let queue_state = QueueState::from_queue(&queue);

    // Save to database in a single transaction
    if let Ok(db) = state.db.try_lock() {
        let _ = save_queue(
            &db,
            queue_state.items.as_slice(),
            queue_state.current_index,
            queue_state.shuffle,
            queue_state.repeat_mode,
        );
    }

    state.events.queue_changed(queue_state);
}

fn load_queue_items_for_song_ids(
    state: &DesktopState,
    song_ids: &[String],
) -> AppResult<Vec<QueueItem>> {
    if song_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = vec!["?"; song_ids.len()].join(", ");
    let query = format!(
        "SELECT s.id, s.title, a.name, al.name, s.duration
         FROM songs s
         LEFT JOIN artists a ON s.artist_id = a.id
         LEFT JOIN albums al ON s.album_id = al.id
         WHERE s.id IN ({placeholders})"
    );

    let conn = state.db.lock_recover();
    let mut stmt = conn.prepare(&query)?;
    let items_by_id = stmt
        .query_map(params_from_iter(song_ids.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                QueueItem {
                    song_id: row.get(0)?,
                    title: row.get(1)?,
                    artist: row
                        .get::<_, Option<String>>(2)?
                        .unwrap_or_else(|| "Unknown Artist".to_string()),
                    album: row
                        .get::<_, Option<String>>(3)?
                        .unwrap_or_else(|| "Unknown Album".to_string()),
                    duration: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                },
            ))
        })?
        .collect::<Result<HashMap<_, _>, _>>()?;

    let mut items = Vec::with_capacity(song_ids.len());
    for song_id in song_ids {
        if let Some(item) = items_by_id.get(song_id) {
            items.push(item.clone());
        }
    }

    Ok(items)
}

pub fn get_queue(state: &DesktopState) -> QueueState {
    let mut queue = state.queue.lock_recover();
    queue.prepare_next_cycle_if_needed();
    QueueState::from_queue(&queue)
}

pub async fn play_song_with_queue(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
    song_id: String,
    song_ids: Vec<String>,
) -> AppResult<()> {
    if song_ids.is_empty() {
        return Err(AppError::Audio(
            "Cannot play from an empty queue".to_string(),
        ));
    }

    if state
        .navigating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }

    let result = async {
        let queue_items = load_queue_items_for_song_ids(&state, &song_ids)?;
        let current_index = queue_items
            .iter()
            .position(|item| item.song_id == song_id)
            .ok_or_else(|| AppError::Audio("Selected song is not available".to_string()))?;

        {
            let mut queue = state.queue.lock_recover();
            *queue = PlayQueue::load(queue_items, Some(current_index), false, RepeatMode::Off);
        }

        persist_and_emit(&state);
        crate::operations::playback::play_song_by_id(runtime, Arc::clone(&state), &song_id).await
    }
    .await;

    state.navigating.store(false, Ordering::SeqCst);
    result
}

pub fn add_to_queue(state: &DesktopState, item: QueueItem) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.add(item);
    }
    persist_and_emit(state);
    Ok(())
}

pub fn add_songs_to_queue(state: &DesktopState, items: Vec<QueueItem>) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.add_many(items);
    }
    persist_and_emit(state);
    Ok(())
}

pub fn insert_next_in_queue(state: &DesktopState, item: QueueItem) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.insert_next(item);
    }
    persist_and_emit(state);
    Ok(())
}

pub fn insert_next_songs_in_queue(state: &DesktopState, items: Vec<QueueItem>) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.insert_many_next(items);
    }
    persist_and_emit(state);
    Ok(())
}

pub fn remove_from_queue(state: &DesktopState, index: usize) -> AppResult<Option<QueueItem>> {
    let result = {
        let mut queue = state.queue.lock_recover();
        queue.remove(index)
    };
    persist_and_emit(state);
    Ok(result)
}

pub fn clear_queue(state: &DesktopState) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.clear();
    }
    persist_and_emit(state);
    Ok(())
}

pub fn move_queue_item(state: &DesktopState, from: usize, to: usize) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.move_item(from, to);
    }
    persist_and_emit(state);
    Ok(())
}

pub fn reroll_next_queue_item(state: &DesktopState) -> AppResult<bool> {
    let swapped = {
        let mut queue = state.queue.lock_recover();
        queue.reroll_next()
    };

    if swapped {
        persist_and_emit(state);
    }

    Ok(swapped)
}

pub async fn play_queue_item(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
    index: usize,
) -> AppResult<()> {
    // Prevent race condition: if already navigating, skip this request
    if state
        .navigating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(());
    }

    let result = async {
        let song_id = {
            let mut queue = state.queue.lock_recover();
            queue.set_current(index).map(|item| item.song_id.clone())
        };

        // Persist current index change
        persist_and_emit(&state);

        if let Some(song_id) = song_id {
            crate::operations::playback::play_song_by_id(runtime, Arc::clone(&state), &song_id)
                .await?;
        }

        Ok(())
    }
    .await;

    // Always release the navigation lock
    state.navigating.store(false, Ordering::SeqCst);
    result
}

pub async fn play_next(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
    force: Option<bool>,
) -> AppResult<bool> {
    // Prevent race condition: if already navigating, skip this request
    if state
        .navigating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(false);
    }

    let result = async {
        let next_song = {
            let mut queue = state.queue.lock_recover();
            queue
                .next(force.unwrap_or(false))
                .map(|item| item.song_id.clone())
        };

        // Persist current index change
        persist_and_emit(&state);

        if let Some(song_id) = next_song {
            // Check if crossfade should be used (only when currently playing)
            let should_crossfade = {
                let audio_player = state.audio_player.lock_recover();
                let status = audio_player.get_status();
                if status.is_playing {
                    let settings = read_playback_settings(&state.settings);
                    settings.crossfade_enabled && settings.crossfade_on_manual_queue_advance
                } else {
                    false
                }
            };

            if should_crossfade {
                let settings = read_playback_settings(&state.settings);
                crate::operations::playback::crossfade_play_by_id(
                    runtime,
                    Arc::clone(&state),
                    &song_id,
                    settings.crossfade_duration_ms,
                )
                .await?;
            } else {
                crate::operations::playback::play_song_by_id(runtime, Arc::clone(&state), &song_id)
                    .await?;
            }
            Ok(true)
        } else {
            state.events.queue_ended();
            Ok(false)
        }
    }
    .await;

    // Always release the navigation lock
    state.navigating.store(false, Ordering::SeqCst);
    result
}

pub async fn play_previous(
    runtime: &tokio::runtime::Handle,
    state: Arc<DesktopState>,
) -> AppResult<bool> {
    // Prevent race condition: if already navigating, skip this request
    if state
        .navigating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(false);
    }

    let result = async {
        let prev_song = {
            let mut queue = state.queue.lock_recover();
            queue.previous().map(|item| item.song_id.clone())
        };

        // Persist current index change
        persist_and_emit(&state);

        if let Some(song_id) = prev_song {
            crate::operations::playback::play_song_by_id(runtime, Arc::clone(&state), &song_id)
                .await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    .await;

    // Always release the navigation lock
    state.navigating.store(false, Ordering::SeqCst);
    result
}

pub fn toggle_shuffle(state: &DesktopState) -> bool {
    let shuffle = {
        let mut queue = state.queue.lock_recover();
        queue.toggle_shuffle();
        queue.is_shuffle()
    };
    persist_and_emit(state);
    shuffle
}

pub fn set_repeat_mode(state: &DesktopState, mode: RepeatMode) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.set_repeat_mode(mode);
    }
    persist_and_emit(state);
    Ok(())
}

pub fn cycle_repeat_mode(state: &DesktopState) -> RepeatMode {
    let mode = {
        let mut queue = state.queue.lock_recover();
        queue.cycle_repeat_mode()
    };
    persist_and_emit(state);
    mode
}
