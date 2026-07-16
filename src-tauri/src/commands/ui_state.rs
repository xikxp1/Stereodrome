use stereodrome_desktop::DesktopBackend;
use tauri::{AppHandle, Manager};

use crate::commands::windowing::MiniPlayerPosition;
use crate::error::{AppError, AppResult};

const KEY_VOLUME: &str = "volume";
const KEY_MINI_PLAYER_POSITION: &str = "mini_player_position";
const DEFAULT_VOLUME: f32 = 0.8;

fn normalized_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        DEFAULT_VOLUME
    }
}

pub fn read_persisted_volume(app_handle: &AppHandle) -> f32 {
    app_handle
        .state::<DesktopBackend>()
        .ui_state()
        .get::<f32>(KEY_VOLUME)
        .ok()
        .flatten()
        .map(normalized_volume)
        .unwrap_or(DEFAULT_VOLUME)
}

pub fn write_persisted_volume(app_handle: &AppHandle, volume: f32) -> AppResult<()> {
    app_handle
        .state::<DesktopBackend>()
        .ui_state()
        .set(KEY_VOLUME, normalized_volume(volume))?;
    Ok(())
}

fn is_finite_position(position: &MiniPlayerPosition) -> bool {
    position.x.is_finite() && position.y.is_finite()
}

pub fn read_mini_player_position(app_handle: &AppHandle) -> Option<MiniPlayerPosition> {
    app_handle
        .state::<DesktopBackend>()
        .ui_state()
        .get::<MiniPlayerPosition>(KEY_MINI_PLAYER_POSITION)
        .ok()
        .flatten()
        .filter(is_finite_position)
}

pub fn write_mini_player_position(
    app_handle: &AppHandle,
    position: MiniPlayerPosition,
) -> AppResult<()> {
    if !is_finite_position(&position) {
        return Err(AppError::Window(
            "mini player position must contain finite coordinates".to_string(),
        ));
    }

    app_handle
        .state::<DesktopBackend>()
        .ui_state()
        .set(KEY_MINI_PLAYER_POSITION, position)?;
    Ok(())
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
