use std::path::Path;

use log::warn;
use stereodrome_core::backup::{
    BackupSummary, PortablePreferences, import_into_connection, read_from_file, write_to_file,
};
use stereodrome_core::{AudioProcessingSettings, ConnectivitySettings, SyncSettings};
use tauri::{AppHandle, Emitter, State};

use crate::audio::binaural::BinauralPreset;
use crate::audio::compressor::DynamicsPreset;
use crate::audio::queue::PlayQueue;
use crate::commands::library::{LibraryMutationGuard, rebuild_search_index_from_db};
use crate::commands::settings::{
    ConnectivitySettings as DesktopConnectivitySettings, NormalizationMode,
    SyncSettings as DesktopSyncSettings, read_connectivity_settings, read_normalization_settings,
    read_playback_settings, read_sync_settings, write_connectivity_settings,
    write_normalization_settings, write_playback_settings, write_sync_settings,
};
use crate::commands::ui_state::{read_persisted_volume, write_persisted_volume};
use crate::error::{AppError, AppResult, MutexExt};
use crate::state::AppState;

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn export_portable_backup(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<BackupSummary> {
    let _library_guard = LibraryMutationGuard::acquire().ok_or_else(|| {
        AppError::Backup("wait for library sync to finish before exporting".to_string())
    })?;
    let preferences = desktop_preferences(&app_handle);
    let _queue = state.queue.lock_recover();
    let mut db = state.db.lock_recover();
    let backup = stereodrome_core::backup::export_from_connection(&mut db, preferences)
        .map_err(|error| backup_error(&error))?;
    write_to_file(Path::new(&path), &backup).map_err(|error| backup_error(&error))?;
    Ok(backup.summary())
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn import_portable_backup(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> AppResult<BackupSummary> {
    let backup = read_from_file(Path::new(&path)).map_err(|error| backup_error(&error))?;
    let _library_guard = LibraryMutationGuard::acquire().ok_or_else(|| {
        AppError::Backup("wait for library sync to finish before importing".to_string())
    })?;
    state.audio_player.lock_recover().stop()?;

    let summary = {
        let mut queue = state.queue.lock_recover();
        let mut db = state.db.lock_recover();
        let summary =
            import_into_connection(&mut db, &backup).map_err(|error| backup_error(&error))?;
        *queue = PlayQueue::load_with_original_order(
            backup.queue.items.clone(),
            backup.queue.original_items.clone(),
            backup.queue.current_index,
            backup.queue.shuffle,
            backup.queue.repeat_mode,
        );
        summary
    };

    if let Err(error) = apply_desktop_preferences(&app_handle, &backup.preferences) {
        warn!("Backup data imported but preferences could not be fully applied: {error}");
    }
    if let Some(volume) = backup.preferences.volume
        && let Err(error) = state
            .audio_player
            .lock_recover()
            .set_volume(f64_to_f32(volume))
    {
        warn!("Backup data imported but audio volume could not be applied: {error}");
    }
    if let Err(error) = rebuild_search_index_from_db(&state) {
        warn!("Backup data imported but search index could not be rebuilt: {error}");
    }
    emit_import_events(&app_handle, &state);
    Ok(summary)
}

fn desktop_preferences(app_handle: &AppHandle) -> PortablePreferences {
    let normalization = read_normalization_settings(app_handle);
    let playback = read_playback_settings(app_handle);
    let sync = read_sync_settings(app_handle);
    let connectivity = read_connectivity_settings(app_handle);
    PortablePreferences {
        sync: Some(SyncSettings {
            incremental_enabled: sync.incremental_enabled,
            incremental_interval_minutes: sync.incremental_interval_minutes,
            full_reconcile_enabled: sync.full_reconcile_enabled,
            full_reconcile_interval_hours: sync.full_reconcile_interval_hours,
        }),
        connectivity: Some(ConnectivitySettings {
            manual_offline_enabled: connectivity.manual_offline_enabled,
        }),
        audio_processing: Some(AudioProcessingSettings {
            normalization_enabled: normalization.enabled,
            normalization_mode: match normalization.mode {
                NormalizationMode::Track => "track",
                NormalizationMode::Album => "album",
            }
            .to_string(),
            target_lufs: normalization.target_lufs,
            preamp_db: normalization.pre_amp_db,
            prevent_clipping: normalization.prevent_clipping,
            dynamics_enabled: normalization.dynamics_enabled,
            dynamics_preset: dynamics_name(&normalization.dynamics_preset).to_string(),
            binaural_enabled: playback.binaural_enabled,
            binaural_preset: binaural_name(&playback.binaural_preset).to_string(),
            equalizer_enabled: playback.equalizer_enabled,
            equalizer_bands_db: playback
                .equalizer_bands_db
                .iter()
                .map(|value| f64::from(*value))
                .collect(),
            gapless_enabled: playback.gapless_enabled,
            crossfade_enabled: playback.crossfade_enabled,
            crossfade_duration_ms: playback.crossfade_duration_ms,
            prefetch_count: playback.prefetch_count,
        }),
        volume: Some(f64::from(read_persisted_volume(app_handle))),
    }
}

fn apply_desktop_preferences(
    app_handle: &AppHandle,
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
        normalization.mode = if audio.normalization_mode == "album" {
            NormalizationMode::Album
        } else {
            NormalizationMode::Track
        };
        normalization.target_lufs = audio.target_lufs.clamp(-30.0, 0.0);
        normalization.pre_amp_db = audio.preamp_db.clamp(-10.0, 10.0);
        normalization.prevent_clipping = audio.prevent_clipping;
        normalization.dynamics_enabled = audio.dynamics_enabled;
        normalization.dynamics_preset = parse_dynamics(&audio.dynamics_preset);
        write_normalization_settings(app_handle, &normalization)?;

        let mut playback = read_playback_settings(app_handle);
        playback.gapless_enabled = audio.gapless_enabled;
        playback.crossfade_enabled = audio.crossfade_enabled;
        playback.crossfade_duration_ms = audio.crossfade_duration_ms.clamp(1000, 12_000);
        playback.binaural_enabled = audio.binaural_enabled;
        playback.binaural_preset = parse_binaural(&audio.binaural_preset);
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
        write_persisted_volume(app_handle, f64_to_f32(volume))?;
    }
    Ok(())
}

fn emit_import_events(app_handle: &AppHandle, state: &AppState) {
    let _ = crate::commands::queue::persist_and_emit(state, app_handle);
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

fn dynamics_name(preset: &DynamicsPreset) -> &'static str {
    match preset {
        DynamicsPreset::Light => "light",
        DynamicsPreset::Medium => "medium",
        DynamicsPreset::Heavy => "heavy",
    }
}

fn parse_dynamics(value: &str) -> DynamicsPreset {
    match value {
        "heavy" => DynamicsPreset::Heavy,
        "medium" => DynamicsPreset::Medium,
        _ => DynamicsPreset::Light,
    }
}

fn binaural_name(preset: &BinauralPreset) -> &'static str {
    match preset {
        BinauralPreset::Aggressive => "strong",
        BinauralPreset::Jmeier => "medium",
        BinauralPreset::Default | BinauralPreset::Cmoy => "light",
    }
}

fn parse_binaural(value: &str) -> BinauralPreset {
    match value {
        "strong" => BinauralPreset::Aggressive,
        "medium" => BinauralPreset::Jmeier,
        _ => BinauralPreset::Default,
    }
}

#[allow(clippy::cast_possible_truncation)]
fn f64_to_f32(value: f64) -> f32 {
    value as f32
}

fn backup_error(error: &stereodrome_core::CoreError) -> AppError {
    AppError::Backup(error.to_string())
}
