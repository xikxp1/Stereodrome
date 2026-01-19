use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::audio::queue::{QueueItem, RepeatMode};
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct QueueState {
    pub items: Vec<QueueItem>,
    pub current_index: Option<usize>,
    pub shuffle: bool,
    pub repeat_mode: RepeatMode,
}

#[tauri::command]
pub fn get_queue(state: State<'_, AppState>) -> QueueState {
    let queue = state.queue.lock().unwrap();
    QueueState {
        items: queue.items().to_vec(),
        current_index: queue.current_index(),
        shuffle: queue.is_shuffle(),
        repeat_mode: queue.repeat_mode(),
    }
}

#[tauri::command]
pub fn add_to_queue(state: State<'_, AppState>, item: QueueItem) -> AppResult<()> {
    let mut queue = state.queue.lock().unwrap();
    queue.add(item);
    Ok(())
}

#[tauri::command]
pub fn add_songs_to_queue(state: State<'_, AppState>, items: Vec<QueueItem>) -> AppResult<()> {
    let mut queue = state.queue.lock().unwrap();
    queue.add_many(items);
    Ok(())
}

#[tauri::command]
pub fn insert_next_in_queue(state: State<'_, AppState>, item: QueueItem) -> AppResult<()> {
    let mut queue = state.queue.lock().unwrap();
    queue.insert_next(item);
    Ok(())
}

#[tauri::command]
pub fn remove_from_queue(state: State<'_, AppState>, index: usize) -> AppResult<Option<QueueItem>> {
    let mut queue = state.queue.lock().unwrap();
    Ok(queue.remove(index))
}

#[tauri::command]
pub fn clear_queue(state: State<'_, AppState>) -> AppResult<()> {
    let mut queue = state.queue.lock().unwrap();
    queue.clear();
    Ok(())
}

#[tauri::command]
pub fn move_queue_item(state: State<'_, AppState>, from: usize, to: usize) -> AppResult<()> {
    let mut queue = state.queue.lock().unwrap();
    queue.move_item(from, to);
    Ok(())
}

#[tauri::command]
pub async fn play_queue_item(
    state: State<'_, AppState>,
    _app_handle: AppHandle,
    index: usize,
) -> AppResult<()> {
    let song_id = {
        let mut queue = state.queue.lock().unwrap();
        queue.set_current(index).map(|item| item.song_id.clone())
    };

    if let Some(song_id) = song_id {
        // Use the existing play_song logic
        crate::commands::play_song(state, song_id).await?;
    }

    Ok(())
}

#[tauri::command]
pub async fn play_next(state: State<'_, AppState>, app_handle: AppHandle) -> AppResult<bool> {
    let next_song = {
        let mut queue = state.queue.lock().unwrap();
        queue.next().map(|item| item.song_id.clone())
    };

    if let Some(song_id) = next_song {
        crate::commands::play_song(state, song_id).await?;
        Ok(true)
    } else {
        // No more songs, emit ended event
        let _ = app_handle.emit("queue-ended", ());
        Ok(false)
    }
}

#[tauri::command]
pub async fn play_previous(state: State<'_, AppState>, _app_handle: AppHandle) -> AppResult<bool> {
    let prev_song = {
        let mut queue = state.queue.lock().unwrap();
        queue.previous().map(|item| item.song_id.clone())
    };

    if let Some(song_id) = prev_song {
        crate::commands::play_song(state, song_id).await?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub fn toggle_shuffle(state: State<'_, AppState>) -> bool {
    let mut queue = state.queue.lock().unwrap();
    queue.toggle_shuffle();
    queue.is_shuffle()
}

#[tauri::command]
pub fn set_repeat_mode(state: State<'_, AppState>, mode: RepeatMode) -> AppResult<()> {
    let mut queue = state.queue.lock().unwrap();
    queue.set_repeat_mode(mode);
    Ok(())
}

#[tauri::command]
pub fn cycle_repeat_mode(state: State<'_, AppState>) -> RepeatMode {
    let mut queue = state.queue.lock().unwrap();
    queue.cycle_repeat_mode()
}
