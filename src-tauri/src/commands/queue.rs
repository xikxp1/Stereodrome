use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::audio::queue::{PlayQueue, QueueItem, RepeatMode};
use crate::db::queue::save_queue;
use crate::error::{AppResult, MutexExt};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct QueueState {
    pub items: Vec<QueueItem>,
    pub current_index: Option<usize>,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
    pub pending_navigation_index: Option<usize>,
}

impl QueueState {
    fn from_queue(queue: &PlayQueue) -> Self {
        Self {
            items: queue.items().to_vec(),
            current_index: queue.current_index(),
            shuffle: queue.is_shuffle(),
            repeat_mode: queue.repeat_mode(),
            pending_navigation_index: queue.pending_navigation_index(),
        }
    }
}

/// Save queue to database and emit queue-changed event
fn persist_and_emit(state: &AppState, app_handle: &AppHandle) {
    let queue = state.queue.lock_recover();
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

    // Emit event to frontend
    let _ = app_handle.emit("queue-changed", &queue_state);
}

#[tauri::command]
pub fn get_queue(state: State<'_, AppState>) -> QueueState {
    let queue = state.queue.lock_recover();
    QueueState::from_queue(&queue)
}

#[tauri::command]
pub fn add_to_queue(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    item: QueueItem,
) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.add(item);
    }
    persist_and_emit(&state, &app_handle);
    Ok(())
}

#[tauri::command]
pub fn add_songs_to_queue(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    items: Vec<QueueItem>,
) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.add_many(items);
    }
    persist_and_emit(&state, &app_handle);
    Ok(())
}

#[tauri::command]
pub fn insert_next_in_queue(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    item: QueueItem,
) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.insert_next(item);
    }
    persist_and_emit(&state, &app_handle);
    Ok(())
}

#[tauri::command]
pub fn remove_from_queue(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    index: usize,
) -> AppResult<Option<QueueItem>> {
    let result = {
        let mut queue = state.queue.lock_recover();
        queue.remove(index)
    };
    persist_and_emit(&state, &app_handle);
    Ok(result)
}

#[tauri::command]
pub fn clear_queue(state: State<'_, AppState>, app_handle: AppHandle) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.clear();
    }
    persist_and_emit(&state, &app_handle);
    Ok(())
}

#[tauri::command]
pub fn move_queue_item(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    from: usize,
    to: usize,
) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.move_item(from, to);
    }
    persist_and_emit(&state, &app_handle);
    Ok(())
}

#[tauri::command]
pub async fn play_queue_item(
    state: State<'_, AppState>,
    app_handle: AppHandle,
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
        persist_and_emit(&state, &app_handle);

        if let Some(song_id) = song_id {
            crate::commands::play_song(app_handle, state.clone(), song_id).await?;
        }

        Ok(())
    }
    .await;

    // Always release the navigation lock
    state.navigating.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub async fn play_next(
    state: State<'_, AppState>,
    app_handle: AppHandle,
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
        persist_and_emit(&state, &app_handle);

        if let Some(song_id) = next_song {
            crate::commands::play_song(app_handle.clone(), state.clone(), song_id).await?;
            Ok(true)
        } else {
            let _ = app_handle.emit("queue-ended", ());
            Ok(false)
        }
    }
    .await;

    // Always release the navigation lock
    state.navigating.store(false, Ordering::SeqCst);
    result
}

#[tauri::command]
pub async fn play_previous(state: State<'_, AppState>, app_handle: AppHandle) -> AppResult<bool> {
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
        persist_and_emit(&state, &app_handle);

        if let Some(song_id) = prev_song {
            crate::commands::play_song(app_handle, state.clone(), song_id).await?;
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

#[tauri::command]
pub fn toggle_shuffle(state: State<'_, AppState>, app_handle: AppHandle) -> bool {
    let shuffle = {
        let mut queue = state.queue.lock_recover();
        queue.toggle_shuffle();
        queue.is_shuffle()
    };
    persist_and_emit(&state, &app_handle);
    shuffle
}

#[tauri::command]
pub fn set_repeat_mode(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    mode: RepeatMode,
) -> AppResult<()> {
    {
        let mut queue = state.queue.lock_recover();
        queue.set_repeat_mode(mode);
    }
    persist_and_emit(&state, &app_handle);
    Ok(())
}

#[tauri::command]
pub fn cycle_repeat_mode(state: State<'_, AppState>, app_handle: AppHandle) -> RepeatMode {
    let mode = {
        let mut queue = state.queue.lock_recover();
        queue.cycle_repeat_mode()
    };
    persist_and_emit(&state, &app_handle);
    mode
}
