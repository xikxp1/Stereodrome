use rusqlite::Row;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectParams {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSettingsUpdate {
    pub url: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub connected: bool,
    pub server_url: Option<String>,
    pub username: Option<String>,
    pub server_version: Option<String>,
}

impl ConnectionStatus {
    pub fn disconnected() -> Self {
        Self {
            connected: false,
            server_url: None,
            username: None,
            server_version: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub album_count: i32,
    pub cover_art_id: Option<String>,
    pub synced_at: String,
}

impl Artist {
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncResult {
    pub artists: usize,
    pub albums: usize,
    pub songs: usize,
}

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

    pub fn clamped(mut self) -> Self {
        self.clamp();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanStatus {
    pub scanning: bool,
    pub count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultSong {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultAlbum {
    pub id: String,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<i32>,
    pub song_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultArtist {
    pub id: String,
    pub name: String,
    pub album_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub songs: Vec<SearchResultSong>,
    pub albums: Vec<SearchResultAlbum>,
    pub artists: Vec<SearchResultArtist>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySyncStatus {
    pub active_job: Option<String>,
    pub full: SyncJobStatus,
    pub incremental: SyncJobStatus,
    pub full_reconcile: SyncJobStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_size: u64,
    pub file_count: u64,
    pub max_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadStatus {
    pub song_id: String,
    pub cached: bool,
    pub path: Option<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPlaylistOfflineResult {
    pub playlist_id: String,
    pub saved_offline: bool,
    pub downloaded_count: i32,
    pub removed_count: i32,
    pub skipped_protected_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackState {
    pub current_song_id: Option<String>,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    pub was_playing: bool,
    pub app_volume: f64,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackProgress {
    pub song_id: String,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    pub is_playing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioProcessingSettings {
    pub normalization_enabled: bool,
    pub normalization_mode: String,
    pub target_lufs: f64,
    pub preamp_db: f64,
    pub prevent_clipping: bool,
    pub dynamics_enabled: bool,
    pub dynamics_preset: String,
    pub binaural_enabled: bool,
    pub binaural_preset: String,
    pub equalizer_enabled: bool,
    pub equalizer_bands_db: Vec<f64>,
    pub gapless_enabled: bool,
    pub crossfade_enabled: bool,
    pub crossfade_duration_ms: u32,
}

impl Default for AudioProcessingSettings {
    fn default() -> Self {
        Self {
            normalization_enabled: false,
            normalization_mode: "track".to_string(),
            target_lufs: -14.0,
            preamp_db: 0.0,
            prevent_clipping: true,
            dynamics_enabled: false,
            dynamics_preset: "light".to_string(),
            binaural_enabled: false,
            binaural_preset: "medium".to_string(),
            equalizer_enabled: false,
            equalizer_bands_db: vec![0.0; 12],
            gapless_enabled: true,
            crossfade_enabled: false,
            crossfade_duration_ms: 5000,
        }
    }
}
