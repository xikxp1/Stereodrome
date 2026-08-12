use std::path::Path;

use log::warn;
use stereodrome_core::backup::{BackupSummary, PortablePreferences, read_from_file};
use stereodrome_core::{
    BinauralPreset as CoreBinauralPreset, CoreCommand, DynamicsPreset as CoreDynamicsPreset,
    NormalizationMode as CoreNormalizationMode,
};
use tauri::{AppHandle, Emitter, State};

use crate::audio::binaural::BinauralPreset;
use crate::audio::compressor::DynamicsPreset;
use crate::commands::settings::{
    ConnectivitySettings as DesktopConnectivitySettings, NormalizationMode,
    SyncSettings as DesktopSyncSettings, read_connectivity_settings, read_normalization_settings,
    read_playback_settings, read_sync_settings, write_connectivity_settings,
    write_normalization_settings, write_playback_settings, write_sync_settings,
};
use crate::error::{AppError, AppResult};
use crate::runtime::{dispatch, dispatch_unit};
use crate::state::AppState;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn import_portable_backup(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<BackupSummary> {
    let backup = read_from_file(Path::new(&path)).map_err(|error| backup_error(&error))?;
    let summary = dispatch(&state, CoreCommand::ImportPortableBackup { path })?;

    if let Err(error) = apply_desktop_preferences(&app_handle, &state, &backup.preferences) {
        warn!("Backup data imported but preferences could not be fully applied: {error}");
    }
    emit_import_events(&app_handle);
    Ok(summary)
}

fn apply_desktop_preferences(
    app_handle: &AppHandle,
    state: &AppState,
    preferences: &PortablePreferences,
) -> AppResult<()> {
    if let Some(sync) = &preferences.sync {
        write_sync_settings(
            app_handle,
            &DesktopSyncSettings {
                incremental_enabled: sync.incremental_enabled,
                incremental_interval_minutes: sync.incremental_interval_minutes.clamp(5, 720),
                full_reconcile_enabled: sync.full_reconcile_enabled,
                full_reconcile_interval_hours: sync.full_reconcile_interval_hours.clamp(1, 168),
            },
        )?;
    }
    if let Some(connectivity) = &preferences.connectivity {
        write_connectivity_settings(
            app_handle,
            &DesktopConnectivitySettings {
                manual_offline_enabled: connectivity.manual_offline_enabled,
            },
        )?;
    }
    if let Some(audio) = &preferences.audio_processing {
        let mut normalization = read_normalization_settings(app_handle);
        normalization.enabled = audio.normalization_enabled;
        normalization.mode = match audio.normalization_mode {
            CoreNormalizationMode::Track => NormalizationMode::Track,
            CoreNormalizationMode::Album => NormalizationMode::Album,
        };
        normalization.target_lufs = audio.target_lufs.clamp(-30.0, 0.0);
        normalization.pre_amp_db = audio.preamp_db.clamp(-10.0, 10.0);
        normalization.prevent_clipping = audio.prevent_clipping;
        normalization.dynamics_enabled = audio.dynamics_enabled;
        normalization.dynamics_preset = parse_dynamics(audio.dynamics_preset);
        write_normalization_settings(app_handle, &normalization)?;

        let mut playback = read_playback_settings(app_handle);
        playback.gapless_enabled = audio.gapless_enabled;
        playback.crossfade_enabled = audio.crossfade_enabled;
        playback.crossfade_duration_ms = audio.crossfade_duration_ms.clamp(1000, 12_000);
        playback.binaural_enabled = audio.binaural_enabled;
        playback.binaural_preset = parse_binaural(audio.binaural_preset);
        playback.equalizer_enabled = audio.equalizer_enabled;
        playback.equalizer_bands_db = audio
            .equalizer_bands_db
            .iter()
            .map(|value| f64_to_f32(*value))
            .collect();
        playback.prefetch_count = audio.prefetch_count.clamp(1, 10);
        write_playback_settings(app_handle, &playback)?;
    }
    if let Some(volume) = preferences.volume {
        dispatch_unit(
            state,
            CoreCommand::SetPlaybackVolume {
                volume: f64_to_f32(volume),
            },
        )?;
    }
    Ok(())
}

fn emit_import_events(app_handle: &AppHandle) {
    let _ = app_handle.emit(
        "playback-settings-changed",
        read_playback_settings(app_handle),
    );
    let _ = app_handle.emit(
        "connectivity-settings-changed",
        read_connectivity_settings(app_handle),
    );
    let _ = app_handle.emit("sync-settings-changed", read_sync_settings(app_handle));
}

fn parse_dynamics(value: CoreDynamicsPreset) -> DynamicsPreset {
    match value {
        CoreDynamicsPreset::Light => DynamicsPreset::Light,
        CoreDynamicsPreset::Medium => DynamicsPreset::Medium,
        CoreDynamicsPreset::Heavy => DynamicsPreset::Heavy,
    }
}

fn parse_binaural(value: CoreBinauralPreset) -> BinauralPreset {
    match value {
        CoreBinauralPreset::Strong => BinauralPreset::Aggressive,
        CoreBinauralPreset::Medium => BinauralPreset::Jmeier,
        CoreBinauralPreset::Light => BinauralPreset::Default,
    }
}

#[allow(clippy::cast_possible_truncation)]
fn f64_to_f32(value: f64) -> f32 {
    value as f32
}

fn backup_error(error: &stereodrome_core::CoreError) -> AppError {
    AppError::Backup(error.to_string())
}
