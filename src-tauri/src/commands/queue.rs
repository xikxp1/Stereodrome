use stereodrome_desktop::DesktopBackend;
use stereodrome_desktop::audio::queue::{QueueItem, RepeatMode};
use stereodrome_desktop::operations::queue::QueueState;
use tauri::State;

use crate::error::AppResult;

#[tauri::command]
pub fn get_queue(backend: State<'_, DesktopBackend>) -> QueueState {
    stereodrome_desktop::operations::queue::get_queue(&backend.state())
}

#[tauri::command]
pub async fn play_song_with_queue(
    backend: State<'_, DesktopBackend>,
    song_id: String,
    song_ids: Vec<String>,
) -> AppResult<()> {
    stereodrome_desktop::operations::queue::play_song_with_queue(
        &backend.runtime_handle(),
        backend.state(),
        song_id,
        song_ids,
    )
    .await
}

#[tauri::command]
pub fn add_to_queue(backend: State<'_, DesktopBackend>, item: QueueItem) -> AppResult<()> {
    stereodrome_desktop::operations::queue::add_to_queue(&backend.state(), item)
}

#[tauri::command]
pub fn add_songs_to_queue(
    backend: State<'_, DesktopBackend>,
    items: Vec<QueueItem>,
) -> AppResult<()> {
    stereodrome_desktop::operations::queue::add_songs_to_queue(&backend.state(), items)
}

#[tauri::command]
pub fn insert_next_in_queue(backend: State<'_, DesktopBackend>, item: QueueItem) -> AppResult<()> {
    stereodrome_desktop::operations::queue::insert_next_in_queue(&backend.state(), item)
}

#[tauri::command]
pub fn insert_next_songs_in_queue(
    backend: State<'_, DesktopBackend>,
    items: Vec<QueueItem>,
) -> AppResult<()> {
    stereodrome_desktop::operations::queue::insert_next_songs_in_queue(&backend.state(), items)
}

#[tauri::command]
pub fn remove_from_queue(
    backend: State<'_, DesktopBackend>,
    index: usize,
) -> AppResult<Option<QueueItem>> {
    stereodrome_desktop::operations::queue::remove_from_queue(&backend.state(), index)
}

#[tauri::command]
pub fn clear_queue(backend: State<'_, DesktopBackend>) -> AppResult<()> {
    stereodrome_desktop::operations::queue::clear_queue(&backend.state())
}

#[tauri::command]
pub fn move_queue_item(
    backend: State<'_, DesktopBackend>,
    from: usize,
    to: usize,
) -> AppResult<()> {
    stereodrome_desktop::operations::queue::move_queue_item(&backend.state(), from, to)
}

#[tauri::command]
pub fn reroll_next_queue_item(backend: State<'_, DesktopBackend>) -> AppResult<bool> {
    stereodrome_desktop::operations::queue::reroll_next_queue_item(&backend.state())
}

#[tauri::command]
pub async fn play_queue_item(backend: State<'_, DesktopBackend>, index: usize) -> AppResult<()> {
    stereodrome_desktop::operations::queue::play_queue_item(
        &backend.runtime_handle(),
        backend.state(),
        index,
    )
    .await
}

#[tauri::command]
pub async fn play_next(backend: State<'_, DesktopBackend>, force: Option<bool>) -> AppResult<bool> {
    stereodrome_desktop::operations::queue::play_next(
        &backend.runtime_handle(),
        backend.state(),
        force,
    )
    .await
}

#[tauri::command]
pub async fn play_previous(backend: State<'_, DesktopBackend>) -> AppResult<bool> {
    stereodrome_desktop::operations::queue::play_previous(
        &backend.runtime_handle(),
        backend.state(),
    )
    .await
}

#[tauri::command]
pub fn toggle_shuffle(backend: State<'_, DesktopBackend>) -> bool {
    stereodrome_desktop::operations::queue::toggle_shuffle(&backend.state())
}

#[tauri::command]
pub fn set_repeat_mode(backend: State<'_, DesktopBackend>, mode: RepeatMode) -> AppResult<()> {
    stereodrome_desktop::operations::queue::set_repeat_mode(&backend.state(), mode)
}

#[tauri::command]
pub fn cycle_repeat_mode(backend: State<'_, DesktopBackend>) -> RepeatMode {
    stereodrome_desktop::operations::queue::cycle_repeat_mode(&backend.state())
}
