use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

use crate::commands::windowing::MiniPlayerPosition;
use crate::error::{AppError, AppResult};

const STORE_FILE: &str = "state.json";
const KEY_VOLUME: &str = "volume";
const KEY_MINI_PLAYER_POSITION: &str = "mini_player_position";
const DEFAULT_VOLUME: f32 = 0.8;

fn to_io_error(context: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Io(std::io::Error::other(format!("{context}: {err}")))
}

fn normalized_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        DEFAULT_VOLUME
    }
}

pub(crate) fn take_legacy_persisted_volume(app_handle: &AppHandle) -> Option<f32> {
    let store = app_handle.store(STORE_FILE).ok()?;
    let volume = store
        .get(KEY_VOLUME)
        .and_then(|value| serde_json::from_value::<f32>(value.clone()).ok())
        .map(normalized_volume)?;
    store.delete(KEY_VOLUME);
    let _ = store.save();
    Some(volume)
}

pub fn write_persisted_volume(app_handle: &AppHandle, volume: f32) -> AppResult<()> {
    let volume = normalized_volume(volume);
    let store = app_handle
        .store(STORE_FILE)
        .map_err(|e| to_io_error("failed to open runtime state store", e))?;
    store.set(KEY_VOLUME, serde_json::json!(volume));
    store
        .save()
        .map_err(|e| to_io_error("failed to save runtime state store", e))?;
    Ok(())
}

fn is_finite_position(position: &MiniPlayerPosition) -> bool {
    position.x.is_finite() && position.y.is_finite()
}

pub fn read_mini_player_position(app_handle: &AppHandle) -> Option<MiniPlayerPosition> {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Some(value) = store.get(KEY_MINI_PLAYER_POSITION)
        && let Ok(position) = serde_json::from_value::<MiniPlayerPosition>(value.clone())
        && is_finite_position(&position)
    {
        return Some(position);
    }

    None
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

    let store = app_handle
        .store(STORE_FILE)
        .map_err(|e| to_io_error("failed to open runtime state store", e))?;
    let value = serde_json::to_value(position)
        .map_err(|e| to_io_error("failed to serialize mini player position", e))?;
    store.set(KEY_MINI_PLAYER_POSITION, value);
    store
        .save()
        .map_err(|e| to_io_error("failed to save runtime state store", e))?;
    Ok(())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_persisted_volume(app_handle: AppHandle, volume: f32) -> AppResult<()> {
    write_persisted_volume(&app_handle, volume)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_mini_player_position(app_handle: AppHandle) -> Option<MiniPlayerPosition> {
    read_mini_player_position(&app_handle)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn set_mini_player_position(
    app_handle: AppHandle,
    position: MiniPlayerPosition,
) -> AppResult<()> {
    write_mini_player_position(&app_handle, position)
}
