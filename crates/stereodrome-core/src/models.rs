use rusqlite::Row;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectParams {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettingsUpdate {
    #[cfg_attr(feature = "ts", ts(optional))]
    pub url: Option<String>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub username: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub connected: bool,
    pub server_url: Option<String>,
    pub username: Option<String>,
    pub server_version: Option<String>,
}

impl ConnectionStatus {
    #[must_use]
    pub fn disconnected() -> Self {
        Self {
            connected: false,
            server_url: None,
            username: None,
            server_version: None,
        }
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub album_count: i32,
    pub cover_art_id: Option<String>,
    pub synced_at: String,
}

impl Artist {
    /// Builds an artist from the columns in a library query row.
    ///
    /// # Errors
    ///
    /// Returns an error when a required column is missing or has an incompatible type.
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            album_count: row.get(2)?,
            cover_art_id: row.get(3)?,
            synced_at: row.get(4)?,
        })
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Album {
    pub id: String,
    pub artist_id: String,
    pub name: String,
    pub year: Option<i32>,
    pub song_count: i32,
    pub duration: Option<i32>,
    pub cover_art_id: Option<String>,
    pub synced_at: String,
    pub artist_name: Option<String>,
}

impl Album {
    /// Builds an album from the columns in a library query row.
    ///
    /// # Errors
    ///
    /// Returns an error when a required column is missing or has an incompatible type.
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            artist_id: row.get(1)?,
            name: row.get(2)?,
            year: row.get(3)?,
            song_count: row.get(4)?,
            duration: row.get(5)?,
            cover_art_id: row.get(6)?,
            synced_at: row.get(7)?,
            artist_name: row.get(8)?,
        })
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumListEntry {
    pub id: String,
    pub name: String,
    pub artist_id: Option<String>,
    pub artist_name: Option<String>,
    pub year: Option<i32>,
    pub song_count: Option<i32>,
    pub duration: Option<i32>,
    pub cover_art_id: Option<String>,
    pub play_count: Option<i64>,
    pub created: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Song {
    pub id: String,
    pub album_id: String,
    pub artist_id: String,
    pub title: String,
    pub track_number: Option<i32>,
    pub disc_number: i32,
    pub duration: Option<i32>,
    pub bit_rate: Option<i32>,
    pub size: Option<i64>,
    pub suffix: Option<String>,
    pub content_type: Option<String>,
    pub path: Option<String>,
    pub year: Option<i32>,
    pub genre: Option<String>,
    pub synced_at: String,
    pub artist: Option<String>,
    pub album: Option<String>,
}

impl Song {
    /// Builds a song from the columns in a library query row.
    ///
    /// # Errors
    ///
    /// Returns an error when a required column is missing or has an incompatible type.
    pub fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            album_id: row.get(1)?,
            artist_id: row.get(2)?,
            title: row.get(3)?,
            track_number: row.get(4)?,
            disc_number: row.get(5)?,
            duration: row.get(6)?,
            bit_rate: row.get(7)?,
            size: row.get(8)?,
            suffix: row.get(9)?,
            content_type: row.get(10)?,
            path: row.get(11)?,
            year: row.get(12)?,
            genre: row.get(13)?,
            synced_at: row.get(14)?,
            artist: row.get(15)?,
            album: row.get(16)?,
        })
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub song_count: i32,
    pub duration: i32,
    pub owner: Option<String>,
    pub cover_art_id: Option<String>,
    pub created_at: String,
    pub changed_at: String,
    pub saved_offline: bool,
    pub offline_saved_at: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncResult {
    pub artists: usize,
    pub albums: usize,
    pub songs: usize,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

fn default_true() -> bool {
    true
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

impl SyncSettings {
    pub fn clamp(&mut self) {
        self.incremental_interval_minutes = self.incremental_interval_minutes.clamp(5, 720);
        self.full_reconcile_interval_hours = self.full_reconcile_interval_hours.clamp(1, 168);
    }

    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.clamp();
        self
    }
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectivitySettings {
    #[serde(default)]
    pub manual_offline_enabled: bool,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanStatus {
    pub scanning: bool,
    pub count: Option<i64>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NowPlayingEntry {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<i32>,
    pub cover_art: Option<String>,
    pub username: String,
    pub minutes_ago: i32,
    pub player_name: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultSong {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<i32>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultAlbum {
    pub id: String,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<i32>,
    pub song_count: i32,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultArtist {
    pub id: String,
    pub name: String,
    pub album_count: i32,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub songs: Vec<SearchResultSong>,
    pub albums: Vec<SearchResultAlbum>,
    pub artists: Vec<SearchResultArtist>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncJobStatus {
    pub enabled: bool,
    pub interval_minutes: u32,
    pub running: bool,
    pub last_attempt_at: Option<String>,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub next_run_at: Option<String>,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySyncStatus {
    pub active_job: Option<String>,
    pub full: SyncJobStatus,
    pub incremental: SyncJobStatus,
    pub full_reconcile: SyncJobStatus,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_size: u64,
    pub file_count: u64,
    pub max_size: u64,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadStatus {
    pub song_id: String,
    pub cached: bool,
    pub path: Option<String>,
    pub bytes: u64,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPlaylistOfflineResult {
    pub playlist_id: String,
    pub saved_offline: bool,
    pub downloaded_count: i32,
    pub removed_count: i32,
    pub skipped_protected_count: i32,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackState {
    pub current_song_id: Option<String>,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    pub was_playing: bool,
    pub app_volume: f64,
    pub updated_at: String,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackProgress {
    pub song_id: String,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    pub is_playing: bool,
}

/// Falls back to the default variant instead of rejecting the whole payload when
/// a persisted setting or imported backup carries an unrecognized value.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NormalizationMode {
    #[default]
    Track,
    Album,
}

#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DynamicsPreset {
    #[default]
    Light,
    Medium,
    Heavy,
}

/// Widths exposed in settings; mapped onto concrete crossfeed presets when the
/// audio graph is built.
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BinauralPreset {
    Light,
    #[default]
    Medium,
    Strong,
}

// These booleans are stable, independently configurable serialized settings.
#[allow(clippy::struct_excessive_bools)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioProcessingSettings {
    pub normalization_enabled: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub normalization_mode: NormalizationMode,
    pub target_lufs: f64,
    pub preamp_db: f64,
    pub prevent_clipping: bool,
    pub dynamics_enabled: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub dynamics_preset: DynamicsPreset,
    pub binaural_enabled: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub binaural_preset: BinauralPreset,
    pub equalizer_enabled: bool,
    pub equalizer_bands_db: Vec<f64>,
    pub gapless_enabled: bool,
    pub crossfade_enabled: bool,
    pub crossfade_duration_ms: u32,
    #[serde(default = "default_prefetch_count")]
    pub prefetch_count: u32,
}

fn default_prefetch_count() -> u32 {
    3
}

impl Default for AudioProcessingSettings {
    fn default() -> Self {
        Self {
            normalization_enabled: false,
            normalization_mode: NormalizationMode::Track,
            target_lufs: -14.0,
            preamp_db: 0.0,
            prevent_clipping: true,
            dynamics_enabled: false,
            dynamics_preset: DynamicsPreset::Light,
            binaural_enabled: false,
            binaural_preset: BinauralPreset::Medium,
            equalizer_enabled: false,
            equalizer_bands_db: vec![0.0; 12],
            gapless_enabled: true,
            crossfade_enabled: false,
            crossfade_duration_ms: 5000,
            prefetch_count: default_prefetch_count(),
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test setup and assertions intentionally fail fast"
)]
mod tests {
    use super::{AudioProcessingSettings, BinauralPreset, DynamicsPreset, NormalizationMode};

    /// Persisted settings and imported backups may carry preset values this build
    /// does not know. Only the unrecognized field falls back; the rest survives.
    #[test]
    fn unknown_preset_values_fall_back_without_discarding_other_settings() {
        let json = serde_json::json!({
            "normalization_enabled": true,
            "normalization_mode": "loudness-war",
            "target_lufs": -18.0,
            "preamp_db": 3.0,
            "prevent_clipping": true,
            "dynamics_enabled": true,
            "dynamics_preset": "crushing",
            "binaural_enabled": true,
            "binaural_preset": "cavernous",
            "equalizer_enabled": true,
            "equalizer_bands_db": [1.0],
            "gapless_enabled": false,
            "crossfade_enabled": true,
            "crossfade_duration_ms": 4000,
            "prefetch_count": 5
        });

        let settings: AudioProcessingSettings =
            serde_json::from_value(json).expect("unknown presets do not fail the payload");

        assert_eq!(settings.normalization_mode, NormalizationMode::Track);
        assert_eq!(settings.dynamics_preset, DynamicsPreset::Light);
        assert_eq!(settings.binaural_preset, BinauralPreset::Medium);
        assert!((settings.target_lufs + 18.0).abs() < f64::EPSILON);
        assert_eq!(settings.crossfade_duration_ms, 4000);
        assert!(!settings.gapless_enabled);
    }

    #[test]
    fn preset_values_round_trip_through_their_serialized_names() {
        let settings = AudioProcessingSettings {
            normalization_mode: NormalizationMode::Album,
            dynamics_preset: DynamicsPreset::Heavy,
            binaural_preset: BinauralPreset::Strong,
            ..Default::default()
        };

        let json = serde_json::to_value(&settings).expect("settings serialize");
        assert_eq!(json["normalization_mode"], "album");
        assert_eq!(json["dynamics_preset"], "heavy");
        assert_eq!(json["binaural_preset"], "strong");

        let restored: AudioProcessingSettings =
            serde_json::from_value(json).expect("settings deserialize");
        assert_eq!(restored.normalization_mode, NormalizationMode::Album);
        assert_eq!(restored.dynamics_preset, DynamicsPreset::Heavy);
        assert_eq!(restored.binaural_preset, BinauralPreset::Strong);
    }
}
