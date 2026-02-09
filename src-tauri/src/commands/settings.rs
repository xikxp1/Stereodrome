use log::warn;
use tauri::{AppHandle, State};
use tauri_plugin_store::StoreExt;

use crate::audio::binaural::BinauralPreset;
use crate::audio::compressor::DynamicsPreset;
use crate::audio::equalizer::{default_bands_db, sanitize_bands_db};
use crate::error::AppResult;
use crate::state::AppState;

const STORE_FILE: &str = "settings.json";
const KEY_NORMALIZATION: &str = "normalization";
const KEY_PLAYBACK: &str = "playback";

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NormalizationMode {
    Track,
    Album,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NormalizationSettings {
    pub enabled: bool,
    pub mode: NormalizationMode,
    pub target_lufs: f64,
    pub pre_amp_db: f64,
    pub prevent_clipping: bool,
    #[serde(default)]
    pub dynamics_enabled: bool,
    #[serde(default = "default_dynamics_preset")]
    pub dynamics_preset: DynamicsPreset,
}

fn default_dynamics_preset() -> DynamicsPreset {
    DynamicsPreset::Medium
}

impl Default for NormalizationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: NormalizationMode::Track,
            target_lufs: -14.0,
            pre_amp_db: 0.0,
            prevent_clipping: true,
            dynamics_enabled: false,
            dynamics_preset: DynamicsPreset::Medium,
        }
    }
}

/// Read normalization settings from settings.json
pub fn read_normalization_settings(app_handle: &AppHandle) -> NormalizationSettings {
    if let Ok(store) = app_handle.store(STORE_FILE) {
        if let Some(value) = store.get(KEY_NORMALIZATION) {
            if let Ok(settings) = serde_json::from_value(value.clone()) {
                return settings;
            }
        }
    }
    NormalizationSettings::default()
}

/// Write normalization settings to settings.json
fn write_normalization_settings(app_handle: &AppHandle, settings: &NormalizationSettings) {
    if let Ok(store) = app_handle.store(STORE_FILE) {
        if let Ok(value) = serde_json::to_value(settings) {
            store.set(KEY_NORMALIZATION, value);
            let _ = store.save();
        }
    }
}

#[tauri::command]
pub fn get_normalization_settings(app_handle: AppHandle) -> NormalizationSettings {
    read_normalization_settings(&app_handle)
}

#[tauri::command]
pub async fn set_normalization_settings(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    mut settings: NormalizationSettings,
) -> AppResult<()> {
    settings.target_lufs = settings.target_lufs.clamp(-30.0, 0.0);
    settings.pre_amp_db = settings.pre_amp_db.clamp(-10.0, 10.0);
    write_normalization_settings(&app_handle, &settings);

    if let Err(e) =
        crate::commands::playback::reapply_settings_to_current_song(&app_handle, &state).await
    {
        warn!("Failed to reapply normalization settings to current playback: {e}");
    }

    Ok(())
}

// --- Playback Settings ---

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaybackSettings {
    #[serde(default = "default_true")]
    pub gapless_enabled: bool,
    #[serde(default)]
    pub crossfade_enabled: bool,
    #[serde(default = "default_crossfade_duration")]
    pub crossfade_duration_ms: u32,
    #[serde(default)]
    pub binaural_enabled: bool,
    #[serde(default = "default_binaural_preset")]
    pub binaural_preset: BinauralPreset,
    #[serde(default)]
    pub equalizer_enabled: bool,
    #[serde(default = "default_equalizer_bands_db")]
    pub equalizer_bands_db: Vec<f32>,
}

fn default_crossfade_duration() -> u32 {
    5000
}

fn default_true() -> bool {
    true
}

fn default_binaural_preset() -> BinauralPreset {
    BinauralPreset::Default
}

fn default_equalizer_bands_db() -> Vec<f32> {
    default_bands_db()
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            gapless_enabled: true,
            crossfade_enabled: false,
            crossfade_duration_ms: 5000,
            binaural_enabled: false,
            binaural_preset: BinauralPreset::Default,
            equalizer_enabled: false,
            equalizer_bands_db: default_bands_db(),
        }
    }
}

pub fn read_playback_settings(app_handle: &AppHandle) -> PlaybackSettings {
    if let Ok(store) = app_handle.store(STORE_FILE) {
        if let Some(value) = store.get(KEY_PLAYBACK) {
            if let Ok(settings) = serde_json::from_value(value.clone()) {
                return settings;
            }
        }
    }
    PlaybackSettings::default()
}

fn write_playback_settings(app_handle: &AppHandle, settings: &PlaybackSettings) {
    if let Ok(store) = app_handle.store(STORE_FILE) {
        if let Ok(value) = serde_json::to_value(settings) {
            store.set(KEY_PLAYBACK, value);
            let _ = store.save();
        }
    }
}

#[tauri::command]
pub fn get_playback_settings(app_handle: AppHandle) -> PlaybackSettings {
    read_playback_settings(&app_handle)
}

#[tauri::command]
pub async fn set_playback_settings(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    mut settings: PlaybackSettings,
) -> AppResult<()> {
    settings.crossfade_duration_ms = settings.crossfade_duration_ms.clamp(1000, 12000);
    settings.equalizer_bands_db = sanitize_bands_db(&settings.equalizer_bands_db);
    write_playback_settings(&app_handle, &settings);

    if let Err(e) =
        crate::commands::playback::reapply_settings_to_current_song(&app_handle, &state).await
    {
        warn!("Failed to reapply playback settings to current playback: {e}");
    }

    Ok(())
}
