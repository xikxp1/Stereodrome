use log::warn;
use stereodrome_desktop::DesktopBackend;
use tauri::{AppHandle, Manager, State};

use crate::error::AppResult;

pub use stereodrome_desktop::operations::settings::{
    ConnectivitySettings, NormalizationSettings, NotificationSettings, PlaybackSettings,
    SyncSettings, SystemTimePreferences,
};

pub fn read_normalization_settings(app_handle: &AppHandle) -> NormalizationSettings {
    let backend = app_handle.state::<DesktopBackend>();
    stereodrome_desktop::operations::settings::read_normalization_settings(backend.settings())
}

#[tauri::command]
pub fn get_normalization_settings(app_handle: AppHandle) -> NormalizationSettings {
    read_normalization_settings(&app_handle)
}

#[tauri::command]
pub async fn set_normalization_settings(
    backend: State<'_, DesktopBackend>,
    settings: NormalizationSettings,
) -> AppResult<()> {
    stereodrome_desktop::operations::settings::set_normalization_settings(
        backend.settings(),
        settings,
    )?;
    if let Err(error) = stereodrome_desktop::operations::playback::reapply_settings_to_current_song(
        &backend.runtime_handle(),
        backend.state(),
    )
    .await
    {
        warn!("Failed to reapply normalization settings to current playback: {error}");
    }
    Ok(())
}

pub fn read_notification_settings(app_handle: &AppHandle) -> NotificationSettings {
    let backend = app_handle.state::<DesktopBackend>();
    stereodrome_desktop::operations::settings::read_notification_settings(backend.settings())
}

#[tauri::command]
pub fn get_notification_settings(app_handle: AppHandle) -> NotificationSettings {
    read_notification_settings(&app_handle)
}

#[tauri::command]
pub fn set_notification_settings(
    app_handle: AppHandle,
    settings: NotificationSettings,
) -> AppResult<()> {
    let backend = app_handle.state::<DesktopBackend>();
    stereodrome_desktop::operations::settings::set_notification_settings(
        backend.settings(),
        settings,
    )
}

pub fn read_playback_settings(app_handle: &AppHandle) -> PlaybackSettings {
    let backend = app_handle.state::<DesktopBackend>();
    stereodrome_desktop::operations::settings::read_playback_settings(backend.settings())
}

#[tauri::command]
pub fn get_playback_settings(app_handle: AppHandle) -> PlaybackSettings {
    read_playback_settings(&app_handle)
}

#[tauri::command]
pub async fn set_playback_settings(
    backend: State<'_, DesktopBackend>,
    settings: PlaybackSettings,
) -> AppResult<()> {
    stereodrome_desktop::operations::settings::set_playback_settings(&backend.state(), settings)?;
    if let Err(error) = stereodrome_desktop::operations::playback::reapply_settings_to_current_song(
        &backend.runtime_handle(),
        backend.state(),
    )
    .await
    {
        warn!("Failed to reapply playback settings to current playback: {error}");
    }
    Ok(())
}

pub fn read_connectivity_settings(app_handle: &AppHandle) -> ConnectivitySettings {
    let backend = app_handle.state::<DesktopBackend>();
    stereodrome_desktop::operations::settings::read_connectivity_settings(backend.settings())
}

#[tauri::command]
pub fn get_connectivity_settings(app_handle: AppHandle) -> ConnectivitySettings {
    read_connectivity_settings(&app_handle)
}

#[tauri::command]
pub fn set_connectivity_settings(
    backend: State<'_, DesktopBackend>,
    settings: ConnectivitySettings,
) -> AppResult<ConnectivitySettings> {
    stereodrome_desktop::operations::settings::set_connectivity_settings(&backend.state(), settings)
}

pub fn read_sync_settings(app_handle: &AppHandle) -> SyncSettings {
    let backend = app_handle.state::<DesktopBackend>();
    stereodrome_desktop::operations::settings::read_sync_settings(backend.settings())
}

#[tauri::command]
pub fn get_sync_settings(app_handle: AppHandle) -> SyncSettings {
    read_sync_settings(&app_handle)
}

#[tauri::command]
pub fn set_sync_settings(
    backend: State<'_, DesktopBackend>,
    settings: SyncSettings,
) -> AppResult<()> {
    stereodrome_desktop::operations::settings::set_sync_settings(&backend.state(), settings)?;
    Ok(())
}

#[tauri::command]
pub fn get_system_time_preferences() -> SystemTimePreferences {
    stereodrome_desktop::operations::settings::get_system_time_preferences()
}
