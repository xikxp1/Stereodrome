use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_FILE: &str = "settings.json";
const KEY_NORMALIZATION: &str = "normalization";

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
}

impl Default for NormalizationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: NormalizationMode::Track,
            target_lufs: -14.0,
            pre_amp_db: 0.0,
            prevent_clipping: true,
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
pub fn set_normalization_settings(app_handle: AppHandle, mut settings: NormalizationSettings) {
    settings.target_lufs = settings.target_lufs.clamp(-30.0, 0.0);
    settings.pre_amp_db = settings.pre_amp_db.clamp(-10.0, 10.0);
    write_normalization_settings(&app_handle, &settings);
}
