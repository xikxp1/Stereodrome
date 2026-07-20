use log::warn;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_store::StoreExt;

use crate::audio::binaural::BinauralPreset;
use crate::audio::compressor::DynamicsPreset;
use crate::audio::equalizer::{default_bands_db, sanitize_bands_db};
use crate::error::AppResult;
use crate::state::AppState;

const STORE_FILE: &str = "settings.json";
const KEY_NORMALIZATION: &str = "normalization";
const KEY_PLAYBACK: &str = "playback";
const KEY_NOTIFICATION: &str = "notification";
const KEY_SYNC: &str = "sync";
const KEY_CONNECTIVITY: &str = "connectivity";

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
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Some(value) = store.get(KEY_NORMALIZATION)
        && let Ok(settings) = serde_json::from_value(value.clone())
    {
        return settings;
    }
    NormalizationSettings::default()
}

/// Write normalization settings to settings.json
fn write_normalization_settings(app_handle: &AppHandle, settings: &NormalizationSettings) {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Ok(value) = serde_json::to_value(settings)
    {
        store.set(KEY_NORMALIZATION, value);
        let _ = store.save();
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

// --- Notification Settings ---

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotificationSettings {
    #[serde(default = "default_notifications_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub notify_when_focused: bool,
    #[serde(default = "default_notify_when_miniplayer_open")]
    pub notify_when_miniplayer_open: bool,
}

fn default_notifications_enabled() -> bool {
    true
}

fn default_notify_when_miniplayer_open() -> bool {
    true
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            notify_when_focused: false,
            notify_when_miniplayer_open: true,
        }
    }
}

pub fn read_notification_settings(app_handle: &AppHandle) -> NotificationSettings {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Some(value) = store.get(KEY_NOTIFICATION)
        && let Ok(settings) = serde_json::from_value(value.clone())
    {
        return settings;
    }
    NotificationSettings::default()
}

fn write_notification_settings(app_handle: &AppHandle, settings: &NotificationSettings) {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Ok(value) = serde_json::to_value(settings)
    {
        store.set(KEY_NOTIFICATION, value);
        let _ = store.save();
    }
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
    write_notification_settings(&app_handle, &settings);
    Ok(())
}

// --- Playback Settings ---

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlaybackSettings {
    #[serde(default = "default_true")]
    pub gapless_enabled: bool,
    #[serde(default)]
    pub crossfade_enabled: bool,
    #[serde(default = "default_true")]
    pub crossfade_on_manual_queue_advance: bool,
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
    #[serde(default = "default_show_next_song_in_miniplayer")]
    pub show_next_song_in_miniplayer: bool,
    #[serde(default = "default_prefetch_count")]
    pub prefetch_count: u32,
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

fn default_show_next_song_in_miniplayer() -> bool {
    true
}

fn default_prefetch_count() -> u32 {
    3
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            gapless_enabled: true,
            crossfade_enabled: false,
            crossfade_on_manual_queue_advance: true,
            crossfade_duration_ms: 5000,
            binaural_enabled: false,
            binaural_preset: BinauralPreset::Default,
            equalizer_enabled: false,
            equalizer_bands_db: default_bands_db(),
            show_next_song_in_miniplayer: true,
            prefetch_count: default_prefetch_count(),
        }
    }
}

pub fn read_playback_settings(app_handle: &AppHandle) -> PlaybackSettings {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Some(value) = store.get(KEY_PLAYBACK)
        && let Ok(settings) = serde_json::from_value(value.clone())
    {
        return settings;
    }
    PlaybackSettings::default()
}

fn write_playback_settings(app_handle: &AppHandle, settings: &PlaybackSettings) {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Ok(value) = serde_json::to_value(settings)
    {
        store.set(KEY_PLAYBACK, value);
        let _ = store.save();
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
    settings.prefetch_count = settings.prefetch_count.clamp(1, 10);
    settings.equalizer_bands_db = sanitize_bands_db(&settings.equalizer_bands_db);
    write_playback_settings(&app_handle, &settings);
    let _ = app_handle.emit("playback-settings-changed", &settings);

    if let Err(e) =
        crate::commands::playback::reapply_settings_to_current_song(&app_handle, &state).await
    {
        warn!("Failed to reapply playback settings to current playback: {e}");
    }

    Ok(())
}

// --- Connectivity Settings ---

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ConnectivitySettings {
    #[serde(default)]
    pub manual_offline_enabled: bool,
}

pub fn read_connectivity_settings(app_handle: &AppHandle) -> ConnectivitySettings {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Some(value) = store.get(KEY_CONNECTIVITY)
        && let Ok(settings) = serde_json::from_value(value.clone())
    {
        return settings;
    }
    ConnectivitySettings::default()
}

fn write_connectivity_settings(app_handle: &AppHandle, settings: &ConnectivitySettings) {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Ok(value) = serde_json::to_value(settings)
    {
        store.set(KEY_CONNECTIVITY, value);
        let _ = store.save();
    }
}

pub fn manual_offline_enabled(app_handle: &AppHandle) -> bool {
    read_connectivity_settings(app_handle).manual_offline_enabled
}

#[tauri::command]
pub fn get_connectivity_settings(app_handle: AppHandle) -> ConnectivitySettings {
    read_connectivity_settings(&app_handle)
}

#[tauri::command]
pub fn set_connectivity_settings(
    app_handle: AppHandle,
    settings: ConnectivitySettings,
) -> AppResult<ConnectivitySettings> {
    write_connectivity_settings(&app_handle, &settings);
    let _ = app_handle.emit("connectivity-settings-changed", &settings);
    Ok(settings)
}

// --- Library Sync Settings ---

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncSettings {
    #[serde(default = "default_true")]
    pub incremental_enabled: bool,
    #[serde(default = "default_incremental_interval_minutes")]
    pub incremental_interval_minutes: u32,
    #[serde(default = "default_true")]
    pub full_reconcile_enabled: bool,
    #[serde(default = "default_full_reconcile_interval_hours")]
    pub full_reconcile_interval_hours: u32,
}

fn default_incremental_interval_minutes() -> u32 {
    15
}

fn default_full_reconcile_interval_hours() -> u32 {
    24
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            incremental_enabled: true,
            incremental_interval_minutes: default_incremental_interval_minutes(),
            full_reconcile_enabled: true,
            full_reconcile_interval_hours: default_full_reconcile_interval_hours(),
        }
    }
}

pub fn read_sync_settings(app_handle: &AppHandle) -> SyncSettings {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Some(value) = store.get(KEY_SYNC)
        && let Ok(settings) = serde_json::from_value(value.clone())
    {
        return settings;
    }
    SyncSettings::default()
}

fn write_sync_settings(app_handle: &AppHandle, settings: &SyncSettings) {
    if let Ok(store) = app_handle.store(STORE_FILE)
        && let Ok(value) = serde_json::to_value(settings)
    {
        store.set(KEY_SYNC, value);
        let _ = store.save();
    }
}

#[tauri::command]
pub fn get_sync_settings(app_handle: AppHandle) -> SyncSettings {
    read_sync_settings(&app_handle)
}

#[tauri::command]
pub fn set_sync_settings(app_handle: AppHandle, mut settings: SyncSettings) -> AppResult<()> {
    settings.incremental_interval_minutes = settings.incremental_interval_minutes.clamp(5, 720);
    settings.full_reconcile_interval_hours = settings.full_reconcile_interval_hours.clamp(1, 168);

    write_sync_settings(&app_handle, &settings);
    let _ = app_handle.emit("sync-settings-changed", &settings);
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SystemTimePreferences {
    pub use_24_hour_clock: bool,
    pub locale: Option<String>,
}

#[tauri::command]
pub fn get_system_time_preferences() -> SystemTimePreferences {
    SystemTimePreferences {
        use_24_hour_clock: detect_24_hour_clock().unwrap_or(false),
        locale: detect_system_locale(),
    }
}

fn detect_24_hour_clock() -> Option<bool> {
    #[cfg(target_os = "macos")]
    {
        detect_24_hour_clock_macos()
    }

    #[cfg(target_os = "windows")]
    {
        detect_24_hour_clock_windows()
    }

    #[cfg(target_os = "linux")]
    {
        detect_24_hour_clock_linux()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

fn detect_system_locale() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        detect_system_locale_macos()
    }

    #[cfg(target_os = "windows")]
    {
        detect_system_locale_windows()
    }

    #[cfg(target_os = "linux")]
    {
        detect_system_locale_linux()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn detect_24_hour_clock_macos() -> Option<bool> {
    let swift_script = "import Foundation; if #available(macOS 13.0, *) { let cycle = Locale.current.hourCycle; switch cycle { case .zeroToTwentyThree, .oneToTwentyFour: print(\"24\"); default: print(\"12\") } } else { let format = DateFormatter.dateFormat(fromTemplate: \"j\", options: 0, locale: Locale.current) ?? \"\"; print(format.contains(\"a\") ? \"12\" : \"24\") }";
    let swift_output = std::process::Command::new("swift")
        .args(["-e", swift_script])
        .output()
        .ok();

    if let Some(output) = swift_output
        && output.status.success()
    {
        let value = String::from_utf8(output.stdout).ok()?.trim().to_lowercase();
        match value.as_str() {
            "24" => return Some(true),
            "12" => return Some(false),
            _ => {}
        }
    }

    let output = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleICUForce24HourTime"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?.trim().to_lowercase();
    match value.as_str() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn detect_system_locale_macos() -> Option<String> {
    // Prefer Foundation-resolved locale/region; this respects macOS region overrides.
    let swift_region_script = "import Foundation; if #available(macOS 13.0, *) { let language = Locale.current.language.languageCode?.identifier ?? \"\"; let region = Locale.current.region?.identifier ?? \"\"; if !language.isEmpty && !region.isEmpty { print(\"\\(language)-\\(region)\") } else { print(Locale.current.identifier) } } else { print(Locale.current.identifier) }";
    let swift_region_output = std::process::Command::new("swift")
        .args(["-e", swift_region_script])
        .output()
        .ok();
    if let Some(output) = swift_region_output
        && output.status.success()
    {
        let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
        if let Some(locale) = normalize_macos_locale_to_bcp47(&value) {
            return Some(locale);
        }
    }

    let defaults_output = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLocale"])
        .output()
        .ok();

    if let Some(output) = defaults_output
        && output.status.success()
    {
        let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
        if let Some(locale) = normalize_macos_locale_to_bcp47(&value) {
            return Some(locale);
        }
    }

    let swift_script = "import Foundation; print(Locale.current.identifier)";
    let swift_output = std::process::Command::new("swift")
        .args(["-e", swift_script])
        .output()
        .ok()?;
    if !swift_output.status.success() {
        return None;
    }

    let value = String::from_utf8(swift_output.stdout)
        .ok()?
        .trim()
        .to_string();
    normalize_macos_locale_to_bcp47(&value)
}

#[cfg(target_os = "macos")]
fn normalize_macos_locale_to_bcp47(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let mut parts = input.splitn(2, '@');
    let base_raw = parts.next().unwrap_or_default().trim();
    if base_raw.is_empty() {
        return None;
    }

    let base_parts: Vec<&str> = base_raw
        .split(['_', '-'])
        .filter(|part| !part.trim().is_empty())
        .collect();
    let language = base_parts.first()?.to_lowercase();
    let mut region = base_parts.iter().skip(1).find_map(|part| {
        let trimmed = part.trim();
        if trimmed.len() == 2 && trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
            Some(trimmed.to_uppercase())
        } else {
            None
        }
    });

    let extensions_raw = parts.next().unwrap_or_default();

    let mut extensions: Vec<(String, String)> = Vec::new();
    for segment in extensions_raw.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let mut kv = segment.splitn(2, '=');
        let key = kv.next().unwrap_or_default().trim().to_lowercase();
        let value = kv.next().unwrap_or_default().trim().to_lowercase();
        if value.is_empty() {
            continue;
        }
        if key == "rg" {
            let region_candidate = value.chars().take(2).collect::<String>().to_uppercase();
            if region_candidate.len() == 2
                && region_candidate.chars().all(|c| c.is_ascii_alphabetic())
            {
                region = Some(region_candidate);
            }
            continue;
        }
        let mapped_key = match key.as_str() {
            "calendar" => "ca",
            "numbers" => "nu",
            "collation" => "co",
            "currency" => "cu",
            _ => continue,
        };
        let mapped_value = value.replace('_', "-");
        extensions.push((mapped_key.to_string(), mapped_value));
    }

    let mut locale = language;
    if let Some(region) = region {
        locale.push('-');
        locale.push_str(&region);
    }

    if extensions.is_empty() {
        return Some(locale);
    }

    locale.push_str("-u");
    for (key, value) in extensions {
        locale.push('-');
        locale.push_str(&key);
        locale.push('-');
        locale.push_str(&value);
    }

    Some(locale)
}

#[cfg(target_os = "windows")]
fn detect_24_hour_clock_windows() -> Option<bool> {
    let output = std::process::Command::new("reg")
        .args(["query", r"HKCU\Control Panel\International", "/v", "iTime"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    for line in stdout.lines() {
        if !line.contains("iTime") {
            continue;
        }
        if line.trim_end().ends_with('1') {
            return Some(true);
        }
        if line.trim_end().ends_with('0') {
            return Some(false);
        }
    }

    None
}

#[cfg(target_os = "windows")]
fn detect_system_locale_windows() -> Option<String> {
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[cultureinfo]::CurrentCulture.Name",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(target_os = "linux")]
fn detect_24_hour_clock_linux() -> Option<bool> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "clock-format"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?.to_lowercase();
    if value.contains("24h") {
        return Some(true);
    }
    if value.contains("12h") {
        return Some(false);
    }

    None
}

#[cfg(target_os = "linux")]
fn detect_system_locale_linux() -> Option<String> {
    let raw = std::env::var("LC_TIME")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| std::env::var("LANG").ok())?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let no_encoding = trimmed.split('.').next().unwrap_or(trimmed);
    let no_modifier = no_encoding.split('@').next().unwrap_or(no_encoding);
    let normalized = no_modifier.replace('_', "-");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}
