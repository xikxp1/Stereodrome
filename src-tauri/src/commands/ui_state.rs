use stereodrome_desktop::DesktopBackend;
use tauri::{AppHandle, Manager};

use crate::commands::windowing::MiniPlayerPosition;
use crate::error::AppResult;

pub fn write_persisted_volume(app_handle: &AppHandle, volume: f32) -> AppResult<()> {
    let backend = app_handle.state::<DesktopBackend>();
    stereodrome_desktop::operations::settings::write_persisted_volume(backend.ui_state(), volume)
}

pub fn read_mini_player_position(app_handle: &AppHandle) -> Option<MiniPlayerPosition> {
    let backend = app_handle.state::<DesktopBackend>();
    stereodrome_desktop::operations::settings::read_mini_player_position(backend.ui_state())
}

pub fn write_mini_player_position(
    app_handle: &AppHandle,
    position: MiniPlayerPosition,
) -> AppResult<()> {
    let backend = app_handle.state::<DesktopBackend>();
    stereodrome_desktop::operations::settings::write_mini_player_position(
        backend.ui_state(),
        position,
    )
}

#[tauri::command]
pub fn set_persisted_volume(app_handle: AppHandle, volume: f32) -> AppResult<()> {
    write_persisted_volume(&app_handle, volume)
}

#[tauri::command]
pub fn get_mini_player_position(app_handle: AppHandle) -> Option<MiniPlayerPosition> {
    read_mini_player_position(&app_handle)
}

#[tauri::command]
pub fn set_mini_player_position(
    app_handle: AppHandle,
    position: MiniPlayerPosition,
) -> AppResult<()> {
    write_mini_player_position(&app_handle, position)
}
