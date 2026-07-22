use stereodrome_core::queue::RepeatMode;
use stereodrome_core::{
    CoreCommand, PlaybackNavigation, SharedQueueItem as QueueItem, SharedQueueState as QueueState,
};
use tauri::State;

use crate::error::AppResult;
use crate::runtime::{dispatch, dispatch_async, dispatch_unit_async};
use crate::state::AppState;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_queue(state: State<'_, AppState>) -> AppResult<QueueState> {
    dispatch(&state, CoreCommand::GetQueue)
}

#[tauri::command]
pub async fn play_song_with_queue(
    state: State<'_, AppState>,
    song_id: String,
    song_ids: Vec<String>,
) -> AppResult<()> {
    dispatch_unit_async(&state, CoreCommand::PlaySelection { song_id, song_ids }).await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn add_to_queue(state: State<'_, AppState>, item: QueueItem) -> AppResult<()> {
    dispatch::<QueueState>(&state, CoreCommand::AddToQueue { item }).map(drop)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn add_songs_to_queue(state: State<'_, AppState>, items: Vec<QueueItem>) -> AppResult<()> {
    dispatch::<QueueState>(&state, CoreCommand::AddSongsToQueue { items }).map(drop)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn insert_next_in_queue(state: State<'_, AppState>, item: QueueItem) -> AppResult<()> {
    dispatch::<QueueState>(&state, CoreCommand::InsertNext { item }).map(drop)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn insert_next_songs_in_queue(
    state: State<'_, AppState>,
    items: Vec<QueueItem>,
) -> AppResult<()> {
    dispatch::<QueueState>(&state, CoreCommand::InsertNextSongs { items }).map(drop)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn remove_from_queue(state: State<'_, AppState>, index: usize) -> AppResult<Option<QueueItem>> {
    dispatch(&state, CoreCommand::RemoveFromQueue { index })
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn clear_queue(state: State<'_, AppState>) -> AppResult<()> {
    dispatch::<QueueState>(&state, CoreCommand::ClearPlayback).map(drop)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn move_queue_item(state: State<'_, AppState>, from: usize, to: usize) -> AppResult<()> {
    dispatch::<QueueState>(&state, CoreCommand::MoveQueueItem { from, to }).map(drop)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn reroll_next_queue_item(state: State<'_, AppState>) -> AppResult<bool> {
    dispatch(&state, CoreCommand::RerollNext)
}

#[tauri::command]
pub async fn play_queue_item(state: State<'_, AppState>, index: usize) -> AppResult<()> {
    dispatch_async::<QueueState>(
        &state,
        CoreCommand::NavigatePlayback {
            navigation: PlaybackNavigation::Index { index },
        },
    )
    .await
    .map(drop)
}

#[tauri::command]
pub async fn play_next(state: State<'_, AppState>, force: Option<bool>) -> AppResult<bool> {
    let queue: QueueState = dispatch_async(
        &state,
        CoreCommand::NavigatePlayback {
            navigation: PlaybackNavigation::Next {
                force: force.unwrap_or(false),
            },
        },
    )
    .await?;
    Ok(queue.current_index.is_some())
}

#[tauri::command]
pub async fn play_previous(state: State<'_, AppState>) -> AppResult<bool> {
    let queue: QueueState = dispatch_async(
        &state,
        CoreCommand::NavigatePlayback {
            navigation: PlaybackNavigation::Previous,
        },
    )
    .await?;
    Ok(queue.current_index.is_some())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn toggle_shuffle(state: State<'_, AppState>) -> AppResult<bool> {
    dispatch(&state, CoreCommand::ToggleShuffle)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_repeat_mode(state: State<'_, AppState>, mode: RepeatMode) -> AppResult<()> {
    dispatch::<QueueState>(&state, CoreCommand::SetRepeatMode { mode }).map(drop)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn cycle_repeat_mode(state: State<'_, AppState>) -> AppResult<RepeatMode> {
    dispatch(&state, CoreCommand::CycleRepeatMode)
}
