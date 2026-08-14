pub mod backup;
mod db;
mod error;
mod lastfm;
mod models;
pub mod protocol;
pub mod queue;
pub mod runtime;
mod subsonic;
#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test helpers intentionally use fail-fast setup and direct fixture indexing"
)]
pub mod test_support;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, LazyLock, Mutex, Once, Weak};
use std::time::{Duration, Instant};

use backup::{BackupSummary, PortablePreferences};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use crossbeam_channel::{Receiver as RuntimeCacheEventReceiver, Sender as RuntimeCacheEventSender};
use log::{debug, info, warn};
use num_traits::ToPrimitive;
use queue::{PlayQueue, QueueItem, QueueState, RepeatMode};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use submarine::{Client, api::get_album_list::Order, auth::AuthBuilder};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use tokio::task::JoinSet;
pub use tokio_util::sync::CancellationToken as PrefetchCancellationToken;

pub use db::open_connection;
pub use error::{CoreError, CoreResult};
pub use lastfm::{LastfmAuthStart, LastfmQueueItem, LastfmStatus};
pub use models::*;
pub use protocol::*;
pub use queue::{QueueItem as SharedQueueItem, QueueState as SharedQueueState};
pub use runtime::StereodromeRuntimeHandle;

pub(crate) const API_VERSION: &str = "1.16.1";

/// Subsonic `c` parameter. Servers key per-client state (transcoding profiles, now
/// playing entries) on this, so desktop and mobile must not share one identity.
pub(crate) const CLIENT_NAME: &str = if cfg!(any(target_os = "android", target_os = "ios")) {
    "StereodromeMobile"
} else {
    "StereodromeDesktop"
};
const MOBILE_PLAYBACK_FORMAT: &str = "mp3";
const LARGE_COVER_ART_SIZE: i32 = 512;
const NEWEST_HEAD_ALBUM_KEY: &str = "library_newest_head_album_id";
const NEWEST_ALBUMS_PAGE_SIZE: usize = 200;
const SETTINGS_SYNC_KEY: &str = "settings_sync";
const SETTINGS_CONNECTIVITY_KEY: &str = "settings_connectivity";
const FULL_LAST_ATTEMPT_AT_KEY: &str = "library_reconcile_last_attempt_at";
const FULL_LAST_SUCCESS_AT_KEY: &str = "library_reconcile_last_success_at";
const FULL_LAST_ERROR_KEY: &str = "library_reconcile_last_error";
const INCREMENTAL_LAST_ATTEMPT_AT_KEY: &str = "library_incremental_last_attempt_at";
const INCREMENTAL_LAST_SUCCESS_AT_KEY: &str = "library_incremental_last_success_at";
const INCREMENTAL_LAST_ERROR_KEY: &str = "library_incremental_last_error";
const ARTIST_FETCH_CONCURRENCY: usize = 8;
const ALBUM_FETCH_CONCURRENCY: usize = 12;
const SONG_DOWNLOAD_CONCURRENCY: usize = 2;
const PREFETCH_MAX_ATTEMPTS: usize = 3;
const PREFETCH_RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
const PREFETCH_FAILURE_COOLDOWN: Duration = Duration::from_secs(30);
const PREFETCH_MAX_FAILURE_COOLDOWN: Duration = Duration::from_mins(5);

static INIT_RUSTLS_CRYPTO_PROVIDER: Once = Once::new();
static DOWNLOADS_IN_PROGRESS: LazyLock<Mutex<HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SONG_DOWNLOAD_LOCKS: LazyLock<Mutex<HashMap<String, Weak<AsyncMutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SONG_DOWNLOAD_PERMITS: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(SONG_DOWNLOAD_CONCURRENCY));
static CACHE_MUTATION_LOCK: Mutex<()> = Mutex::new(());

fn song_download_lock(song_id: &str) -> Arc<AsyncMutex<()>> {
    SONG_DOWNLOAD_LOCKS.lock().map_or_else(
        |_| Arc::new(AsyncMutex::new(())),
        |mut locks| {
            locks.retain(|_, lock| lock.strong_count() > 0);
            if let Some(lock) = locks.get(song_id).and_then(Weak::upgrade) {
                return lock;
            }
            let lock = Arc::new(AsyncMutex::new(()));
            locks.insert(song_id.to_string(), Arc::downgrade(&lock));
            lock
        },
    )
}

fn cache_mutation_guard() -> CoreResult<std::sync::MutexGuard<'static, ()>> {
    CACHE_MUTATION_LOCK
        .lock()
        .map_err(|_| CoreError::LockPoisoned)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheStateEvent {
    DownloadingChanged { song_id: String, downloading: bool },
    CachedChanged { song_id: String, cached: bool },
    Reconcile,
}

#[derive(Clone)]
enum CacheStateEventSender {
    External(Sender<CacheStateEvent>),
    Runtime(RuntimeCacheEventSender<CacheStateEvent>),
}

impl CacheStateEventSender {
    fn send(&self, event: CacheStateEvent) {
        match self {
            Self::External(sender) => {
                let _ = sender.send(event);
            }
            Self::Runtime(sender) => {
                let _ = sender.send(event);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuePrefetchPlan {
    pub queue_revision: u64,
    pub current_index: Option<usize>,
    pub song_ids: Vec<String>,
}

#[derive(Debug, Default)]
pub struct QueuePrefetchOutcome {
    pub statuses: Vec<DownloadStatus>,
    pub completed: bool,
}

#[derive(Debug)]
struct PrefetchFailureState {
    consecutive_failures: u32,
    retry_after: Instant,
}

struct DownloadInProgressGuard {
    song_id: String,
    cache_event_senders: Vec<CacheStateEventSender>,
}

impl DownloadInProgressGuard {
    fn new(song_id: &str, cache_event_senders: Vec<CacheStateEventSender>) -> Self {
        let started = if let Ok(mut downloads) = DOWNLOADS_IN_PROGRESS.lock() {
            downloads
                .entry(song_id.to_string())
                .and_modify(|count| *count = count.saturating_add(1))
                .or_insert(1);
            true
        } else {
            false
        };
        if started {
            let event = CacheStateEvent::DownloadingChanged {
                song_id: song_id.to_string(),
                downloading: true,
            };
            for sender in &cache_event_senders {
                sender.send(event.clone());
            }
        }
        Self {
            song_id: song_id.to_string(),
            cache_event_senders,
        }
    }
}

impl Drop for DownloadInProgressGuard {
    fn drop(&mut self) {
        let mut finished = false;
        if let Ok(mut downloads) = DOWNLOADS_IN_PROGRESS.lock()
            && let Some(count) = downloads.get_mut(&self.song_id)
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                downloads.remove(&self.song_id);
                finished = true;
            }
        }
        if finished {
            let event = CacheStateEvent::DownloadingChanged {
                song_id: self.song_id.clone(),
                downloading: false,
            };
            for sender in &self.cache_event_senders {
                sender.send(event.clone());
            }
        }
    }
}

struct DownloadRecordFinalizer {
    db_path: PathBuf,
    song_id: String,
    armed: bool,
}

impl DownloadRecordFinalizer {
    fn new(db_path: &Path, song_id: &str) -> Self {
        Self {
            db_path: db_path.to_path_buf(),
            song_id: song_id.to_string(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for DownloadRecordFinalizer {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Ok(conn) = Connection::open(&self.db_path) else {
            return;
        };
        let _ = conn.execute(
            "UPDATE download_items
             SET status = 'cancelled', path = NULL, bytes = 0,
                 error = 'download interrupted', updated_at = ?1
             WHERE song_id = ?2 AND status = 'downloading'",
            params![Utc::now().to_rfc3339(), self.song_id],
        );
    }
}

fn init_rustls_crypto_provider() {
    INIT_RUSTLS_CRYPTO_PROVIDER.call_once(|| {
        if rustls::crypto::ring::default_provider()
            .install_default()
            .is_ok()
        {
            debug!("Installed Rustls Ring crypto provider");
        } else {
            debug!("Rustls crypto provider was already installed");
        }
    });
}

struct LibrarySyncData {
    artists: Vec<SyncArtistData>,
    albums: Vec<SyncAlbumData>,
    songs: Vec<SyncSongData>,
}

struct SyncArtistData {
    id: String,
    name: String,
    album_count: i32,
    cover_art: Option<String>,
}

struct SyncAlbumData {
    id: String,
    artist_id: String,
    name: String,
    year: Option<i32>,
    song_count: i32,
    duration: Option<i32>,
    cover_art: Option<String>,
}

struct SyncSongData {
    id: String,
    album_id: String,
    artist_id: String,
    title: String,
    track: Option<i32>,
    disc_number: i32,
    duration: Option<i32>,
    bit_rate: Option<i32>,
    size: Option<i64>,
    suffix: Option<String>,
    content_type: Option<String>,
    path: Option<String>,
    year: Option<i32>,
    genre: Option<String>,
}

#[derive(Debug, Clone)]
struct RemoteAlbumSummary {
    id: String,
    name: String,
    year: Option<i32>,
    song_count: i32,
    duration: i32,
    cover_art: Option<String>,
}

#[derive(Debug)]
struct AlbumFetchRequest {
    album_id: String,
    artist_id: String,
    album_year: Option<i32>,
}

#[derive(Debug)]
struct RemoteSong {
    id: String,
    title: String,
    track: Option<i32>,
    disc_number: i32,
    duration: Option<i32>,
    bit_rate: Option<i32>,
    size: Option<i64>,
    suffix: Option<String>,
    content_type: Option<String>,
    path: Option<String>,
    year: Option<i32>,
    genre: Option<String>,
}

#[derive(Debug, Clone)]
struct LocalArtistRow {
    name: String,
    cover_art_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NewestAlbumCandidate {
    album_id: String,
    artist_id: String,
    artist_name: Option<String>,
}

#[derive(Debug, Clone)]
struct NewestAlbumPageEntry {
    id: String,
    artist_id: Option<String>,
    artist_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewestScanStopReason {
    ReachedPreviousHead,
    ExhaustedNewestFeed,
}

#[derive(Debug)]
struct NewestScanResult {
    head_album_id: Option<String>,
    candidates: Vec<NewestAlbumCandidate>,
    stop_reason: NewestScanStopReason,
}

#[derive(Debug, PartialEq, Eq)]
struct NewestPageScanResult {
    candidates: Vec<NewestAlbumCandidate>,
    reached_previous_head: bool,
}

#[derive(Debug)]
struct InvalidatableCache<T> {
    generation: u64,
    value: Option<T>,
}

impl<T> Default for InvalidatableCache<T> {
    fn default() -> Self {
        Self {
            generation: 0,
            value: None,
        }
    }
}

impl<T> InvalidatableCache<T> {
    fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.value = None;
    }

    fn store_if_current(&mut self, generation: u64, value: T) {
        if self.generation == generation {
            self.value = Some(value);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DueSyncJob {
    Incremental,
    FullReconcile,
}

#[derive(Debug)]
pub struct StereodromeCore {
    data_dir: PathBuf,
    db_path: PathBuf,
    config_path: PathBuf,
    server_config: Mutex<Option<ServerConfig>>,
    client: AsyncMutex<Option<Client>>,
    queue: Mutex<PlayQueue>,
    queue_revision: AtomicU64,
    prefetch_failures: Mutex<HashMap<String, PrefetchFailureState>>,
    lastfm_retry_lock: AsyncMutex<()>,
    cache_event_sender: Option<Sender<CacheStateEvent>>,
    runtime_cache_event_sender: Mutex<Option<RuntimeCacheEventSender<CacheStateEvent>>>,
    offline_song_ids_cache: Mutex<InvalidatableCache<Vec<String>>>,
    audio_processing_settings_cache: Mutex<Option<AudioProcessingSettings>>,
    gapless_eligibility_cache: Mutex<InvalidatableCache<((String, String), bool)>>,
}

fn ensure_queue_navigation_matches(
    queue: &PlayQueue,
    expected_current_song_id: Option<&str>,
    target: Option<&QueueItem>,
    expected_target_song_id: &str,
) -> CoreResult<()> {
    let current_song_id = queue.current_item().map(|item| item.song_id.as_str());
    let target_song_id = target.map(|item| item.song_id.as_str());
    if current_song_id != expected_current_song_id
        || target_song_id != Some(expected_target_song_id)
    {
        return Err(CoreError::InvalidInput(
            "queue changed while playback was being prepared".to_string(),
        ));
    }
    Ok(())
}

impl StereodromeCore {
    /// # Errors
    /// Returns an error if required directories, persisted state, or the database cannot be initialized.
    pub fn new(data_dir: impl AsRef<Path>) -> CoreResult<Self> {
        Self::new_inner(data_dir, None)
    }

    /// Creates a core that publishes cache and download state mutations.
    ///
    /// # Errors
    /// Returns an error if required directories, persisted state, or the database cannot be initialized.
    pub fn new_with_cache_events(
        data_dir: impl AsRef<Path>,
        cache_event_sender: Sender<CacheStateEvent>,
    ) -> CoreResult<Self> {
        Self::new_inner(data_dir, Some(cache_event_sender))
    }

    fn new_inner(
        data_dir: impl AsRef<Path>,
        cache_event_sender: Option<Sender<CacheStateEvent>>,
    ) -> CoreResult<Self> {
        init_rustls_crypto_provider();

        let data_dir = data_dir.as_ref();
        info!(
            "Initializing Stereodrome Rust core at {}",
            data_dir.display()
        );
        std::fs::create_dir_all(data_dir)?;
        std::fs::create_dir_all(data_dir.join("cover_art"))?;
        std::fs::create_dir_all(data_dir.join("cover_cache"))?;
        let db_path = data_dir.join("stereodrome.db");
        let config_path = data_dir.join("server_config.json");
        db::init(&db_path)?;
        let server_config = read_server_config(&config_path)?;
        let queue = db::load_queue(&db_path)?;

        Ok(Self {
            data_dir: data_dir.to_path_buf(),
            db_path,
            config_path,
            server_config: Mutex::new(server_config),
            client: AsyncMutex::new(None),
            queue: Mutex::new(queue),
            queue_revision: AtomicU64::new(0),
            prefetch_failures: Mutex::new(HashMap::new()),
            lastfm_retry_lock: AsyncMutex::new(()),
            cache_event_sender,
            runtime_cache_event_sender: Mutex::new(None),
            offline_song_ids_cache: Mutex::new(InvalidatableCache::default()),
            audio_processing_settings_cache: Mutex::new(None),
            gapless_eligibility_cache: Mutex::new(InvalidatableCache::default()),
        })
    }

    /// Drops the memoized offline song set after any mutation of the library,
    /// the download records, or the cached audio files themselves.
    fn invalidate_offline_song_ids_cache(&self) {
        if let Ok(mut cache) = self.offline_song_ids_cache.lock() {
            cache.invalidate();
        }
    }

    fn emit_cache_state_event(&self, event: CacheStateEvent) {
        // Every library or cache mutation funnels through here, making it the
        // single invalidation point for the memoized offline song set.
        self.invalidate_offline_song_ids_cache();
        if let Some(sender) = &self.cache_event_sender {
            let _ = sender.send(event.clone());
        }
        if let Ok(sender) = self.runtime_cache_event_sender.lock()
            && let Some(sender) = sender.as_ref()
        {
            let _ = sender.send(event);
        }
    }

    pub(crate) fn subscribe_cache_state_events(
        &self,
    ) -> RuntimeCacheEventReceiver<CacheStateEvent> {
        let (sender, receiver) = crossbeam_channel::unbounded();
        if let Ok(mut runtime_sender) = self.runtime_cache_event_sender.lock() {
            *runtime_sender = Some(sender);
        }
        receiver
    }

    fn cache_event_senders(&self) -> Vec<CacheStateEventSender> {
        let mut senders = self
            .cache_event_sender
            .iter()
            .cloned()
            .map(CacheStateEventSender::External)
            .collect::<Vec<_>>();
        if let Ok(runtime_sender) = self.runtime_cache_event_sender.lock()
            && let Some(sender) = runtime_sender.as_ref()
        {
            senders.push(CacheStateEventSender::Runtime(sender.clone()));
        }
        senders
    }

    /// # Errors
    /// Returns an error if offline mode is active, the server cannot be reached, or state cannot be persisted.
    pub async fn connect_server(&self, params: ConnectParams) -> CoreResult<ConnectionStatus> {
        if self.manual_offline_enabled()? {
            return Err(CoreError::OfflineMode);
        }

        info!("Connecting to Subsonic server at {}", params.url);
        let client = build_client(&params.url, &params.username, &params.password);
        let ping = client
            .ping()
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;

        let config = ServerConfig {
            url: params.url,
            username: params.username,
            password: params.password,
        };
        write_server_config(&self.config_path, &config)?;
        save_server_row(&self.db_path, &config)?;

        *self.client.lock().await = Some(client);
        *self
            .server_config
            .lock()
            .map_err(|_| CoreError::LockPoisoned)? = Some(config.clone());

        info!(
            "Connected to Subsonic server at {} as {}",
            config.url, config.username
        );
        Ok(ConnectionStatus {
            connected: true,
            server_url: Some(config.url),
            username: Some(config.username),
            server_version: Some(ping.version),
        })
    }

    /// # Errors
    /// Returns an error if saved settings cannot be read or the updated server connection fails.
    pub async fn update_server_settings(
        &self,
        update: ServerSettingsUpdate,
    ) -> CoreResult<ConnectionStatus> {
        let current = self.current_config()?;
        self.connect_server(ConnectParams {
            url: update.url.unwrap_or(current.url),
            username: update.username.unwrap_or(current.username),
            password: current.password,
        })
        .await
    }

    /// # Errors
    /// Returns an error if saved connection state cannot be read or shared state cannot be updated.
    pub async fn restore_session(&self) -> CoreResult<ConnectionStatus> {
        info!("Restoring saved Subsonic session");
        let config = {
            self.server_config
                .lock()
                .map_err(|_| CoreError::LockPoisoned)?
                .clone()
        };

        let Some(config) = config else {
            *self.client.lock().await = None;
            debug!("No saved Subsonic session to restore");
            return Ok(ConnectionStatus::disconnected());
        };

        if self.manual_offline_enabled()? {
            *self.client.lock().await = None;
            return Ok(ConnectionStatus {
                connected: false,
                server_url: Some(config.url),
                username: Some(config.username),
                server_version: None,
            });
        }

        let client = build_client(&config.url, &config.username, &config.password);
        match client.ping().await {
            Ok(ping) => {
                *self.client.lock().await = Some(client);
                info!("Restored Subsonic session for {}", config.username);
                Ok(ConnectionStatus {
                    connected: true,
                    server_url: Some(config.url),
                    username: Some(config.username),
                    server_version: Some(ping.version),
                })
            }
            Err(error) => {
                *self.client.lock().await = None;
                warn!("Failed to restore Subsonic session: {error}");
                Ok(ConnectionStatus {
                    connected: false,
                    server_url: Some(config.url),
                    username: Some(config.username),
                    server_version: None,
                })
            }
        }
    }

    /// # Errors
    /// Returns an error if shared connection state or the saved configuration cannot be cleared.
    pub async fn disconnect_server(&self) -> CoreResult<()> {
        info!("Disconnecting Subsonic server");
        *self.client.lock().await = None;
        *self
            .server_config
            .lock()
            .map_err(|_| CoreError::LockPoisoned)? = None;
        match std::fs::remove_file(&self.config_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    /// Clears the active network client without deleting persisted server configuration.
    pub async fn deactivate_session(&self) {
        *self.client.lock().await = None;
    }

    /// # Errors
    /// Returns an error if the shared server configuration lock is poisoned.
    pub fn get_connection_status(&self) -> CoreResult<ConnectionStatus> {
        let config = self
            .server_config
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?
            .clone();
        Ok(match config {
            Some(config) => ConnectionStatus {
                connected: false,
                server_url: Some(config.url),
                username: Some(config.username),
                server_version: None,
            },
            None => ConnectionStatus::disconnected(),
        })
    }

    /// # Errors
    /// Returns an error if the server cannot be queried or synchronized library data cannot be persisted.
    pub async fn sync_library(&self) -> CoreResult<SyncResult> {
        let result = self.sync_library_inner().await;
        self.emit_cache_state_event(CacheStateEvent::Reconcile);
        result
    }

    async fn sync_library_inner(&self) -> CoreResult<SyncResult> {
        info!("Starting full library sync");
        let client = self.connected_client().await?;
        self.record_sync_attempt("library_full", None)?;
        let sync_data = fetch_full_library_sync_data(&client).await?;
        info!(
            "Applying full library sync: artists={}, albums={}, songs={}",
            sync_data.artists.len(),
            sync_data.albums.len(),
            sync_data.songs.len()
        );

        let newest_head_album_id = fetch_newest_head_album_id(&client).await?;
        let now = Utc::now().to_rfc3339();
        let mut conn = db::open_connection(&self.db_path)?;
        let tx = conn.transaction()?;
        apply_library_sync_data(
            &tx,
            &sync_data.artists,
            &sync_data.albums,
            &sync_data.songs,
            &now,
        )?;

        tx.execute(
            "INSERT OR REPLACE INTO sync_state (key, value, updated_at)
             VALUES ('library_last_success_at', ?1, ?1)",
            [&now],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO sync_state (key, value, updated_at)
             VALUES ('library_full_last_success_at', ?1, ?1)",
            [&now],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO sync_state (key, value, updated_at)
             VALUES ('library_full_last_error', '', ?1)",
            [&now],
        )?;
        if let Some(head_album_id) = newest_head_album_id {
            tx.execute(
                "INSERT OR REPLACE INTO sync_state (key, value, updated_at)
                 VALUES (?1, ?2, ?3)",
                params![NEWEST_HEAD_ALBUM_KEY, head_album_id, &now],
            )?;
        }
        tx.commit()?;
        self.invalidate_gapless_eligibility_cache();
        let result = SyncResult {
            artists: sync_data.artists.len(),
            albums: sync_data.albums.len(),
            songs: sync_data.songs.len(),
        };
        info!(
            "Full library sync complete: artists={}, albums={}, songs={}",
            result.artists, result.albums, result.songs
        );
        Ok(result)
    }

    /// # Errors
    /// Returns an error if incremental synchronization cannot query the server or update persisted state.
    pub async fn sync_library_incremental(&self) -> CoreResult<SyncResult> {
        let result = self.sync_library_incremental_inner().await;
        self.emit_cache_state_event(CacheStateEvent::Reconcile);
        result
    }

    async fn sync_library_incremental_inner(&self) -> CoreResult<SyncResult> {
        info!("Starting incremental library sync");
        self.record_sync_attempt_keyed(
            INCREMENTAL_LAST_ATTEMPT_AT_KEY,
            INCREMENTAL_LAST_ERROR_KEY,
            None,
        )?;
        match self.run_incremental_library_sync().await {
            Ok(result) => {
                self.record_sync_success_keyed(
                    INCREMENTAL_LAST_SUCCESS_AT_KEY,
                    INCREMENTAL_LAST_ERROR_KEY,
                )?;
                info!(
                    "Incremental library sync complete: artists={}, albums={}, songs={}",
                    result.artists, result.albums, result.songs
                );
                Ok(result)
            }
            Err(error) => {
                warn!("Incremental library sync failed: {error}");
                self.record_sync_attempt_keyed(
                    INCREMENTAL_LAST_ATTEMPT_AT_KEY,
                    INCREMENTAL_LAST_ERROR_KEY,
                    Some(error.to_string()),
                )?;
                Err(error)
            }
        }
    }

    /// # Errors
    /// Returns an error if library reconciliation cannot query the server or update persisted state.
    pub async fn reconcile_library(&self) -> CoreResult<SyncResult> {
        let result = self.reconcile_library_inner().await;
        self.emit_cache_state_event(CacheStateEvent::Reconcile);
        result
    }

    async fn reconcile_library_inner(&self) -> CoreResult<SyncResult> {
        info!("Starting full library sync with reconciliation");
        self.record_sync_attempt_keyed(FULL_LAST_ATTEMPT_AT_KEY, FULL_LAST_ERROR_KEY, None)?;

        let result = match self.sync_library().await {
            Ok(result) => {
                let conn = db::open_connection(&self.db_path)?;
                let synced_at = sync_value(&conn, "library_last_success_at")?.ok_or_else(|| {
                    CoreError::InvalidInput(
                        "library sync did not record a success time".to_string(),
                    )
                })?;
                info!("Pruning library rows not refreshed at {synced_at}");
                prune_stale_library_rows(&conn, &synced_at).map(|()| result)
            }
            Err(error) => Err(error),
        };

        match result {
            Ok(result) => {
                self.record_sync_success_keyed(FULL_LAST_SUCCESS_AT_KEY, FULL_LAST_ERROR_KEY)?;
                info!(
                    "Full library sync with reconciliation complete: artists={}, albums={}, songs={}",
                    result.artists, result.albums, result.songs
                );
                Ok(result)
            }
            Err(error) => {
                warn!("Full library sync with reconciliation failed: {error}");
                self.record_sync_attempt_keyed(
                    FULL_LAST_ATTEMPT_AT_KEY,
                    FULL_LAST_ERROR_KEY,
                    Some(error.to_string()),
                )?;
                Err(error)
            }
        }
    }

    /// # Errors
    /// Returns an error if synchronization settings cannot be read from the database.
    pub fn get_sync_settings(&self) -> CoreResult<SyncSettings> {
        let conn = db::open_connection(&self.db_path)?;
        let Some(json) = sync_value(&conn, SETTINGS_SYNC_KEY)? else {
            return Ok(SyncSettings::default());
        };
        let settings = serde_json::from_str::<SyncSettings>(&json)
            .unwrap_or_else(|_| SyncSettings::default())
            .clamped();
        Ok(settings)
    }

    /// # Errors
    /// Returns an error if synchronization settings cannot be serialized or persisted.
    pub fn set_sync_settings(&self, settings: SyncSettings) -> CoreResult<SyncSettings> {
        let settings = settings.clamped();
        let conn = db::open_connection(&self.db_path)?;
        write_sync_value(&conn, SETTINGS_SYNC_KEY, &serde_json::to_string(&settings)?)?;
        Ok(settings)
    }

    /// # Errors
    /// Returns an error if connectivity settings cannot be read from the database.
    pub fn get_connectivity_settings(&self) -> CoreResult<ConnectivitySettings> {
        let conn = db::open_connection(&self.db_path)?;
        let Some(json) = sync_value(&conn, SETTINGS_CONNECTIVITY_KEY)? else {
            return Ok(ConnectivitySettings::default());
        };
        Ok(serde_json::from_str::<ConnectivitySettings>(&json).unwrap_or_default())
    }

    /// # Errors
    /// Returns an error if connectivity settings cannot be serialized or persisted.
    pub fn set_connectivity_settings(
        &self,
        settings: ConnectivitySettings,
    ) -> CoreResult<ConnectivitySettings> {
        let conn = db::open_connection(&self.db_path)?;
        write_sync_value(
            &conn,
            SETTINGS_CONNECTIVITY_KEY,
            &serde_json::to_string(&settings)?,
        )?;
        Ok(settings)
    }

    /// # Errors
    /// Returns an error if connectivity settings cannot be read.
    pub fn manual_offline_enabled(&self) -> CoreResult<bool> {
        Ok(self.get_connectivity_settings()?.manual_offline_enabled)
    }

    /// # Errors
    /// Returns an error if the due job cannot be determined or synchronization fails.
    pub async fn run_due_library_sync(&self) -> CoreResult<Option<String>> {
        if self.manual_offline_enabled()? {
            return Ok(None);
        }

        match self.next_due_library_sync_job()? {
            Some(DueSyncJob::FullReconcile) => {
                self.reconcile_library().await?;
                Ok(Some("full_reconcile".to_string()))
            }
            Some(DueSyncJob::Incremental) => {
                self.sync_library_incremental().await?;
                Ok(Some("incremental".to_string()))
            }
            None => Ok(None),
        }
    }

    /// # Errors
    /// Returns an error if synchronization settings or job state cannot be read.
    pub fn next_due_library_sync_job(&self) -> CoreResult<Option<DueSyncJob>> {
        let settings = self.get_sync_settings()?;
        self.next_due_sync_job(&settings)
    }

    /// # Errors
    /// Returns an error if no server is connected or its scan status request fails.
    pub async fn get_scan_status(&self) -> CoreResult<ScanStatus> {
        let client = self.connected_client().await?;
        let status = client
            .get_scan_status()
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;

        debug!(
            "Subsonic scan status: scanning={}, count={:?}",
            status.scanning, status.count
        );
        Ok(ScanStatus {
            scanning: status.scanning,
            count: status.count,
        })
    }

    /// # Errors
    /// Returns an error if no server is connected or the scan request fails.
    pub async fn start_scan(&self) -> CoreResult<ScanStatus> {
        info!("Starting Subsonic server scan");
        let client = self.connected_client().await?;
        let status = client
            .start_scan()
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;

        info!(
            "Subsonic server scan requested: scanning={}, count={:?}",
            status.scanning, status.count
        );
        Ok(ScanStatus {
            scanning: status.scanning,
            count: status.count,
        })
    }

    /// Returns the server's currently playing entries.
    ///
    /// # Errors
    /// Returns an error if no server is connected or the request fails.
    pub async fn get_now_playing(&self) -> CoreResult<Vec<NowPlayingEntry>> {
        let client = self.connected_client().await?;
        let now_playing = client
            .get_now_playing()
            .await
            .map_err(|error| CoreError::Subsonic(error.to_string()))?;
        Ok(now_playing
            .entry
            .into_iter()
            .map(|entry| NowPlayingEntry {
                id: entry.child.id,
                title: entry.child.title,
                artist: entry.child.artist,
                album: entry.child.album,
                duration: entry.child.duration,
                cover_art: entry.child.cover_art,
                username: entry.username,
                minutes_ago: entry.minutes_ago,
                player_name: entry.player_name,
            })
            .collect())
    }

    /// Imports a legacy platform credential when no shared Last.fm session exists.
    ///
    /// # Errors
    /// Returns an error if the session cannot be read or persisted.
    pub fn import_lastfm_session_if_missing(
        &self,
        username: String,
        session_key: String,
    ) -> CoreResult<()> {
        lastfm::import_session_if_missing(&self.db_path, username, session_key)
    }

    /// # Errors
    /// Returns an error if synchronization settings or persisted job state cannot be read.
    pub fn get_library_sync_status(&self) -> CoreResult<LibrarySyncStatus> {
        let conn = db::open_connection(&self.db_path)?;
        let settings = self.get_sync_settings()?;
        let full = Self::sync_job_status(&conn, "library_full", false, 1440)?;
        let incremental = Self::sync_job_status(
            &conn,
            "library_incremental",
            settings.incremental_enabled,
            settings.incremental_interval_minutes,
        )?;
        let reconcile = Self::sync_job_status(
            &conn,
            "library_reconcile",
            settings.full_reconcile_enabled,
            settings.full_reconcile_interval_hours.saturating_mul(60),
        )?;

        Ok(LibrarySyncStatus {
            active_job: None,
            full,
            incremental,
            full_reconcile: reconcile,
        })
    }

    /// # Errors
    /// Returns an error if artists cannot be read from the database.
    pub fn get_artists(&self) -> CoreResult<Vec<Artist>> {
        let conn = db::open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, name, album_count, cover_art_id, synced_at
             FROM artists ORDER BY name COLLATE NOCASE",
        )?;
        rows_collect(stmt.query_map([], Artist::from_row)?)
    }

    /// # Errors
    /// Returns an error if albums cannot be read from the database.
    pub fn get_albums(&self, artist_id: Option<String>) -> CoreResult<Vec<Album>> {
        let conn = db::open_connection(&self.db_path)?;
        if let Some(artist_id) = artist_id {
            let mut stmt = conn.prepare(
                "SELECT al.id, al.artist_id, al.name, al.year, al.song_count, al.duration,
                        al.cover_art_id, al.synced_at, ar.name
                 FROM albums al
                 LEFT JOIN artists ar ON al.artist_id = ar.id
                 WHERE al.artist_id = ?1
                 ORDER BY COALESCE(al.year, 9999), al.name COLLATE NOCASE",
            )?;
            rows_collect(stmt.query_map([artist_id], Album::from_row)?)
        } else {
            let mut stmt = conn.prepare(
                "SELECT al.id, al.artist_id, al.name, al.year, al.song_count, al.duration,
                        al.cover_art_id, al.synced_at, ar.name
                 FROM albums al
                 LEFT JOIN artists ar ON al.artist_id = ar.id
                 ORDER BY al.name COLLATE NOCASE",
            )?;
            rows_collect(stmt.query_map([], Album::from_row)?)
        }
    }

    /// # Errors
    /// Returns an error if songs cannot be read from the database.
    pub fn get_songs(
        &self,
        album_id: Option<String>,
        artist_id: Option<String>,
    ) -> CoreResult<Vec<Song>> {
        let conn = db::open_connection(&self.db_path)?;
        match (album_id, artist_id) {
            (Some(album_id), _) => {
                let mut stmt = conn.prepare(db::SONG_SELECT_WITH_JOINS.to_owned().as_str())?;
                rows_collect(stmt.query_map([album_id], Song::from_row)?)
            }
            (None, Some(artist_id)) => {
                let mut stmt = conn.prepare(db::SONG_SELECT_BY_ARTIST)?;
                rows_collect(stmt.query_map([artist_id], Song::from_row)?)
            }
            (None, None) => {
                let mut stmt = conn.prepare(db::SONG_SELECT_ALL)?;
                rows_collect(stmt.query_map([], Song::from_row)?)
            }
        }
    }

    /// # Errors
    /// Returns an error if no server is connected, the list type is unsupported, or the request fails.
    pub async fn get_album_list(
        &self,
        list_type: String,
        size: Option<usize>,
        offset: Option<usize>,
    ) -> CoreResult<Vec<AlbumListEntry>> {
        let client = self.connected_client().await?;
        let order = album_list_order(&list_type)?;
        let albums = client
            .get_album_list2(order, size.or(Some(50)), offset.or(Some(0)), None::<String>)
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;

        Ok(albums
            .into_iter()
            .map(|album| AlbumListEntry {
                id: album.id,
                name: if album.title.is_empty() {
                    album.name
                } else {
                    album.title
                },
                artist_id: album.artist_id,
                artist_name: album.artist,
                year: album.year,
                song_count: None,
                duration: album.duration,
                cover_art_id: album.cover_art,
                play_count: album.play_count,
                created: album.created.map(|dt| dt.to_rfc3339()),
            })
            .collect())
    }

    // Strings are retained by value as part of the stable FFI-facing API.
    /// # Errors
    /// Returns an error if the search limit is invalid or the library database cannot be queried.
    #[allow(clippy::needless_pass_by_value)]
    pub fn search_library(&self, query: String, limit: Option<usize>) -> CoreResult<SearchResults> {
        let conn = db::open_connection(&self.db_path)?;
        let like = format!("%{query}%");
        let limit = i64::try_from(limit.unwrap_or(25).min(100)).map_err(|_| {
            CoreError::InvalidInput("search limit exceeds SQLite range".to_string())
        })?;

        let mut song_stmt = conn.prepare(
            "SELECT s.id, s.title, ar.name, al.name, s.duration
             FROM songs s
             LEFT JOIN artists ar ON s.artist_id = ar.id
             LEFT JOIN albums al ON s.album_id = al.id
             WHERE s.title LIKE ?1 OR ar.name LIKE ?1 OR al.name LIKE ?1
             ORDER BY s.title COLLATE NOCASE LIMIT ?2",
        )?;
        let songs = rows_collect(song_stmt.query_map(params![like, limit], |row| {
            Ok(SearchResultSong {
                id: row.get(0)?,
                title: row.get(1)?,
                artist: row.get(2)?,
                album: row.get(3)?,
                duration: row.get(4)?,
            })
        })?)?;

        let mut album_stmt = conn.prepare(
            "SELECT al.id, al.name, ar.name, al.year, al.song_count
             FROM albums al
             LEFT JOIN artists ar ON al.artist_id = ar.id
             WHERE al.name LIKE ?1 OR ar.name LIKE ?1
             ORDER BY al.name COLLATE NOCASE LIMIT ?2",
        )?;
        let albums = rows_collect(album_stmt.query_map(params![like, limit], |row| {
            Ok(SearchResultAlbum {
                id: row.get(0)?,
                name: row.get(1)?,
                artist: row.get(2)?,
                year: row.get(3)?,
                song_count: row.get(4)?,
            })
        })?)?;

        let mut artist_stmt = conn.prepare(
            "SELECT id, name, album_count FROM artists
             WHERE name LIKE ?1
             ORDER BY name COLLATE NOCASE LIMIT ?2",
        )?;
        let artists = rows_collect(artist_stmt.query_map(params![like, limit], |row| {
            Ok(SearchResultArtist {
                id: row.get(0)?,
                name: row.get(1)?,
                album_count: row.get(2)?,
            })
        })?)?;

        Ok(SearchResults {
            songs,
            albums,
            artists,
        })
    }

    /// # Errors
    /// Returns an error if playlists cannot be fetched from the server or persisted locally.
    pub async fn get_playlists(&self) -> CoreResult<Vec<Playlist>> {
        if let Ok(client) = self.connected_client().await {
            let playlists = client
                .get_playlists(None::<String>)
                .await
                .map_err(|e| CoreError::Subsonic(e.to_string()))?;
            let mapped = playlists
                .into_iter()
                .map(playlist_from_subsonic)
                .collect::<Vec<_>>();
            self.save_playlists(&mapped)?;
            return self.get_cached_playlists(false);
        }

        self.get_cached_playlists(true)
    }

    /// # Errors
    /// Returns an error if playlist songs cannot be fetched from the server or persisted locally.
    pub async fn get_playlist_songs(&self, playlist_id: String) -> CoreResult<Vec<Song>> {
        if let Ok(client) = self.connected_client().await {
            let playlist = client
                .get_playlist(&playlist_id)
                .await
                .map_err(|e| CoreError::Subsonic(e.to_string()))?;
            let now = Utc::now().to_rfc3339();
            let songs = playlist
                .entry
                .into_iter()
                .map(|song| Song {
                    id: song.id,
                    album_id: song.album_id.unwrap_or_default(),
                    artist_id: song.artist_id.unwrap_or_default(),
                    title: song.title,
                    track_number: song.track,
                    disc_number: song.disc_number.unwrap_or(1),
                    duration: song.duration,
                    bit_rate: song.bit_rate,
                    size: song.size,
                    suffix: song.suffix,
                    content_type: song.content_type,
                    path: song.path,
                    year: song.year,
                    genre: song.genre,
                    synced_at: now.clone(),
                    artist: song.artist,
                    album: song.album,
                })
                .collect::<Vec<_>>();
            self.save_playlist_songs(&playlist_id, &songs)?;
            if self.playlist_saved_offline(&playlist_id)? {
                let _ = self.download_local_playlist_songs(&playlist_id).await?;
            }
            return Ok(songs);
        }

        self.get_local_playlist_songs(&playlist_id)
    }

    /// # Errors
    /// Returns an error if no server is connected or the playlist cannot be created and cached.
    pub async fn create_playlist(
        &self,
        name: String,
        song_ids: Vec<String>,
    ) -> CoreResult<Playlist> {
        let client = self.connected_client().await?;
        let song_ids = playlist_song_ids_to_add(song_ids, &HashSet::new());
        let playlist = client
            .create_playlist(name, song_ids)
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        let mapped = playlist_from_subsonic(playlist.base);
        self.save_playlists(std::slice::from_ref(&mapped))?;
        self.get_cached_playlist(&mapped.id)
    }

    /// # Errors
    /// Returns an error if no server is connected or the playlist cannot be renamed and refreshed.
    pub async fn rename_playlist(&self, playlist_id: String, name: String) -> CoreResult<()> {
        let client = self.connected_client().await?;
        client
            .update_playlist(
                playlist_id.clone(),
                Some(name.clone()),
                None::<String>,
                None,
                Vec::<String>::new(),
                Vec::new(),
            )
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;

        let conn = db::open_connection(&self.db_path)?;
        conn.execute(
            "UPDATE playlists SET name = ?1, synced_at = ?2 WHERE id = ?3",
            params![name, Utc::now().to_rfc3339(), playlist_id],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error if no server is connected or the playlist cannot be deleted locally and remotely.
    pub async fn delete_playlist(&self, playlist_id: String) -> CoreResult<()> {
        let removed_song_ids = self.playlist_song_ids(&playlist_id)?;
        let client = self.connected_client().await?;
        client
            .delete_playlist(playlist_id.clone())
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        let conn = db::open_connection(&self.db_path)?;
        conn.execute(
            "DELETE FROM playlist_songs WHERE playlist_id = ?1",
            [&playlist_id],
        )?;
        conn.execute("DELETE FROM playlists WHERE id = ?1", [&playlist_id])?;
        drop(conn);
        let _ = self.remove_unprotected_cached_songs(removed_song_ids)?;
        Ok(())
    }

    /// # Errors
    /// Returns an error if no server is connected or playlist membership cannot be updated and refreshed.
    pub async fn add_songs_to_playlist(
        &self,
        playlist_id: String,
        song_ids: Vec<String>,
    ) -> CoreResult<()> {
        let client = self.connected_client().await?;
        let playlist = client
            .get_playlist(&playlist_id)
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        let existing_song_ids: HashSet<String> =
            playlist.entry.iter().map(|song| song.id.clone()).collect();
        let song_ids = playlist_song_ids_to_add(song_ids, &existing_song_ids);

        if song_ids.is_empty() {
            return Ok(());
        }

        client
            .update_playlist(
                playlist_id.clone(),
                None::<String>,
                None::<String>,
                None,
                song_ids,
                Vec::new(),
            )
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        self.refresh_playlist_cache(&playlist_id).await?;
        if self.playlist_saved_offline(&playlist_id)? {
            let _ = self.download_playlist(playlist_id).await?;
        }
        Ok(())
    }

    /// # Errors
    /// Returns an error if no server is connected or playlist membership cannot be updated and refreshed.
    pub async fn remove_songs_from_playlist(
        &self,
        playlist_id: String,
        song_indexes: Vec<i64>,
    ) -> CoreResult<()> {
        let before_song_ids = self.playlist_song_ids(&playlist_id)?;
        let client = self.connected_client().await?;
        client
            .update_playlist(
                playlist_id.clone(),
                None::<String>,
                None::<String>,
                None,
                Vec::<String>::new(),
                song_indexes,
            )
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        self.refresh_playlist_cache(&playlist_id).await?;
        if self.playlist_saved_offline(&playlist_id)? {
            let after_song_ids = self.playlist_song_ids(&playlist_id)?;
            let removed_song_ids = before_song_ids
                .difference(&after_song_ids)
                .cloned()
                .collect::<HashSet<_>>();
            let _ = self.remove_unprotected_cached_songs(removed_song_ids)?;
        }
        Ok(())
    }

    // Strings are retained by value as part of the stable FFI-facing API.
    /// # Errors
    /// Returns an error if cache state cannot be read, offline mode is active, or server settings are unavailable.
    #[allow(clippy::needless_pass_by_value)]
    pub fn get_stream_uri(&self, song_id: String) -> CoreResult<String> {
        if let Some(path) = self.cached_song_path(&song_id)? {
            return Ok(path_to_file_uri(&path));
        }

        if self.manual_offline_enabled()? {
            return Err(CoreError::OfflineMode);
        }

        let config = self.current_config()?;
        Ok(subsonic::signed_url(
            &config,
            "stream",
            &[("id", song_id.as_str()), ("format", MOBILE_PLAYBACK_FORMAT)],
        ))
    }

    /// # Errors
    /// Returns an error if cover art cannot be read from cache or downloaded from the server.
    pub async fn get_cover_art_uri(
        &self,
        cover_art_id: String,
        size: Option<i32>,
    ) -> CoreResult<String> {
        let path = self.get_or_cache_cover_art(&cover_art_id, size).await?;
        self.prefetch_large_cover_art_if_small(&cover_art_id, size);
        Ok(path_to_file_uri(&path))
    }

    /// # Errors
    /// Returns an error if song metadata or its cover art cannot be read or downloaded.
    pub async fn get_song_cover_art_uri(
        &self,
        song_id: String,
        size: Option<i32>,
    ) -> CoreResult<Option<String>> {
        let conn = db::open_connection(&self.db_path)?;
        let cover_art_id = conn
            .query_row(
                "SELECT al.cover_art_id
                 FROM songs s
                 LEFT JOIN albums al ON s.album_id = al.id
                 WHERE s.id = ?1",
                [song_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();

        match cover_art_id {
            Some(id) => self.get_cover_art_uri(id, size).await.map(Some),
            None => Ok(None),
        }
    }

    /// # Errors
    /// Returns an error if song metadata or cached cover art cannot be read.
    pub fn cached_song_cover_art_uri(
        &self,
        song_id: &str,
        size: Option<i32>,
    ) -> CoreResult<Option<String>> {
        let Some(cover_art_id) = self.song_cover_art_id(song_id)? else {
            return Ok(None);
        };
        Ok(self
            .cached_cover_art_path(&cover_art_id, size)?
            .map(|path| path_to_file_uri(&path)))
    }

    /// # Errors
    /// Returns an error if cache settings or cached files cannot be inspected.
    pub fn get_audio_cache_stats(&self) -> CoreResult<CacheStats> {
        let max_size = self.max_cache_size()?;
        let entries = self.audio_cache_entries()?;
        Ok(CacheStats {
            total_size: entries.iter().map(|(_, size)| *size).sum(),
            file_count: u64::try_from(entries.len()).map_err(|_| {
                CoreError::InvalidInput("audio cache entry count does not fit u64".to_string())
            })?,
            max_size,
        })
    }

    /// Waits for an in-progress cache commit or removal to leave its critical section.
    /// Callers signal cancellation before this barrier so no cancelled download can
    /// commit after the barrier returns.
    ///
    /// # Errors
    /// Returns an error if the cache mutation lock is poisoned.
    pub fn cache_mutation_barrier(&self) -> CoreResult<()> {
        drop(cache_mutation_guard()?);
        Ok(())
    }

    /// # Errors
    /// Returns an error if offline song state cannot be read from the database.
    pub fn get_offline_song_ids(&self) -> CoreResult<Vec<String>> {
        let generation = if let Ok(cache) = self.offline_song_ids_cache.lock() {
            if let Some(song_ids) = cache.value.as_ref() {
                return Ok(song_ids.clone());
            }
            Some(cache.generation)
        } else {
            None
        };
        let song_ids = self.compute_offline_song_ids()?;
        if let Some(generation) = generation
            && let Ok(mut cache) = self.offline_song_ids_cache.lock()
        {
            cache.store_if_current(generation, song_ids.clone());
        }
        Ok(song_ids)
    }

    /// Scans the library and download records for playable cached songs. This
    /// stats every candidate cache file, so results are memoized and callers
    /// go through [`Self::get_offline_song_ids`].
    fn compute_offline_song_ids(&self) -> CoreResult<Vec<String>> {
        let conn = db::open_connection(&self.db_path)?;
        let mut stmt = conn.prepare("SELECT id FROM songs ORDER BY title COLLATE NOCASE")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let library_song_ids = rows.collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut stmt = conn.prepare(
            "SELECT song_id, path FROM download_items
             WHERE status = 'downloaded' AND path IS NOT NULL
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut recorded_paths = HashMap::new();
        for row in rows {
            let (song_id, path) = row?;
            recorded_paths.entry(song_id).or_insert(path);
        }
        drop(stmt);

        let cache_dir = self.audio_cache_dir()?;
        let mut song_ids = Vec::new();
        for song_id in library_song_ids {
            let direct_path = cache_dir.join(format!(
                "{}.{}",
                sanitize_file_component(&song_id),
                MOBILE_PLAYBACK_FORMAT
            ));
            let recorded_path = recorded_paths.get(&song_id).map(PathBuf::from);
            let cached = direct_path.exists()
                || recorded_path
                    .is_some_and(|path| path.exists() && is_mobile_playback_cache_path(&path));
            if cached {
                song_ids.push(song_id);
            }
        }

        Ok(song_ids)
    }

    /// Returns whether a song belongs to the synchronized local library.
    ///
    /// # Errors
    /// Returns an error if library state cannot be read from the database.
    pub fn has_library_song(&self, song_id: &str) -> CoreResult<bool> {
        let conn = db::open_connection(&self.db_path)?;
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM songs WHERE id = ?1)",
            [song_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
    }

    /// # Errors
    /// Returns an error if the cache size cannot be persisted or cache eviction fails.
    pub fn set_max_cache_size(&self, max_size: u64) -> CoreResult<CacheStats> {
        let max_size = max_size.clamp(500 * 1024 * 1024, 50 * 1024 * 1024 * 1024);
        self.set_setting("max_cache_size", &max_size.to_string())?;
        self.enforce_audio_cache_limit()?;
        self.get_audio_cache_stats()
    }

    /// # Errors
    /// Returns an error if cached files or their persisted records cannot be removed.
    pub fn clear_audio_cache(&self) -> CoreResult<CacheStats> {
        let _cache_guard = cache_mutation_guard()?;
        let mutation_result = (|| -> CoreResult<()> {
            let active_song_id = self.get_playback_state()?.current_song_id;
            let mut protected_paths = self.protected_audio_cache_paths()?;
            if let Some(active_path) = self.active_playback_cache_path()? {
                protected_paths.insert(active_path);
            }
            for (path, _) in self.audio_cache_entries()? {
                if protected_paths.contains(&path) {
                    continue;
                }
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        warn!(
                            "skipping cached audio file that could not be cleared: {} ({error})",
                            path.display()
                        );
                    }
                }
            }
            let conn = db::open_connection(&self.db_path)?;
            conn.execute(
                "DELETE FROM download_items
                 WHERE song_id NOT IN (
                    SELECT DISTINCT ps.song_id
                    FROM playlist_songs ps
                    JOIN playlists p ON p.id = ps.playlist_id
                    WHERE p.offline_saved_at IS NOT NULL
                )
                AND (?1 IS NULL OR song_id != ?1)",
                [&active_song_id],
            )?;
            Ok(())
        })();
        self.emit_cache_state_event(CacheStateEvent::Reconcile);
        mutation_result?;
        self.get_audio_cache_stats()
    }

    /// # Errors
    /// Returns an error if the song's cache path or metadata cannot be inspected.
    pub fn is_song_cached(&self, song_id: String) -> CoreResult<DownloadStatus> {
        let _cache_guard = cache_mutation_guard()?;
        let path = self.cached_song_path(&song_id)?;
        let bytes = path
            .as_ref()
            .and_then(|path| path.metadata().ok())
            .map_or(0, |metadata| metadata.len());
        Ok(DownloadStatus {
            song_id,
            cached: path.is_some(),
            path: path.as_ref().map(|path| path_to_file_uri(path)),
            bytes,
        })
    }

    /// # Errors
    /// Returns an error if offline mode is active or the song cannot be downloaded, stored, or recorded.
    pub async fn download_song(&self, song_id: String) -> CoreResult<DownloadStatus> {
        self.download_song_cancellable(song_id, None)
            .await?
            .ok_or_else(|| CoreError::InvalidInput("song download was cancelled".to_string()))
    }

    #[allow(clippy::too_many_lines)]
    async fn download_song_cancellable(
        &self,
        song_id: String,
        cancellation: Option<&PrefetchCancellationToken>,
    ) -> CoreResult<Option<DownloadStatus>> {
        let download_lock = song_download_lock(&song_id);
        let download_lock_guard = if let Some(cancellation) = cancellation {
            tokio::select! {
                () = cancellation.cancelled() => return Ok(None),
                guard = download_lock.lock() => guard,
            }
        } else {
            download_lock.lock().await
        };

        let cached_status = {
            let _cache_guard = cache_mutation_guard()?;
            self.cached_song_path(&song_id)?.map(|path| {
                let bytes = path.metadata().map_or(0, |metadata| metadata.len());
                self.emit_cache_state_event(CacheStateEvent::CachedChanged {
                    song_id: song_id.clone(),
                    cached: true,
                });
                DownloadStatus {
                    song_id: song_id.clone(),
                    cached: true,
                    path: Some(path_to_file_uri(&path)),
                    bytes,
                }
            })
        };
        if let Some(status) = cached_status {
            drop(download_lock_guard);
            if let Some(cancellation) = cancellation {
                tokio::select! {
                    () = cancellation.cancelled() => {}
                    () = self.preserve_cached_song_cover_art_if_connected(&song_id) => {}
                }
            } else {
                self.preserve_cached_song_cover_art_if_connected(&song_id)
                    .await;
            }
            return Ok(Some(status));
        }

        if self.manual_offline_enabled()? {
            return Err(CoreError::OfflineMode);
        }

        let download_permit = if let Some(cancellation) = cancellation {
            tokio::select! {
                () = cancellation.cancelled() => return Ok(None),
                permit = SONG_DOWNLOAD_PERMITS.acquire() => permit,
            }
        } else {
            SONG_DOWNLOAD_PERMITS.acquire().await
        }
        .map_err(|_| CoreError::InvalidInput("song download limiter is closed".to_string()))?;

        let download_guard = DownloadInProgressGuard::new(&song_id, self.cache_event_senders());
        let client = self.connected_client().await?;
        let path = self.audio_cache_path(&song_id, MOBILE_PLAYBACK_FORMAT)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        self.record_download(DownloadRecord {
            entity_type: "song",
            entity_id: &song_id,
            song_id: &song_id,
            status: "downloading",
            path: None,
            bytes: 0,
            error: None,
        })?;
        let mut record_finalizer = DownloadRecordFinalizer::new(&self.db_path, &song_id);
        let mut pending_file = PendingAtomicFile::new(&path);
        let stream = client.stream_to_file(
            song_id.clone(),
            None,
            Some(MOBILE_PLAYBACK_FORMAT),
            None,
            None::<String>,
            None,
            None,
            pending_file.path(),
        );
        let stream_result = if let Some(cancellation) = cancellation {
            tokio::select! {
                () = cancellation.cancelled() => None,
                result = stream => Some(result),
            }
        } else {
            Some(stream.await)
        };

        match stream_result {
            None => {
                self.record_download(DownloadRecord {
                    entity_type: "song",
                    entity_id: &song_id,
                    song_id: &song_id,
                    status: "cancelled",
                    path: None,
                    bytes: 0,
                    error: Some("prefetch cancelled"),
                })?;
                record_finalizer.disarm();
                Ok(None)
            }
            Some(Ok(byte_count)) => {
                {
                    let _cache_guard = cache_mutation_guard()?;
                    if cancellation.is_some_and(PrefetchCancellationToken::is_cancelled) {
                        self.record_download(DownloadRecord {
                            entity_type: "song",
                            entity_id: &song_id,
                            song_id: &song_id,
                            status: "cancelled",
                            path: None,
                            bytes: 0,
                            error: Some("prefetch cancelled before cache commit"),
                        })?;
                        record_finalizer.disarm();
                        return Ok(None);
                    }
                    pending_file.commit(&path)?;
                    self.emit_cache_state_event(CacheStateEvent::CachedChanged {
                        song_id: song_id.clone(),
                        cached: true,
                    });
                    self.record_download(DownloadRecord {
                        entity_type: "song",
                        entity_id: &song_id,
                        song_id: &song_id,
                        status: "downloaded",
                        path: Some(&path),
                        bytes: byte_count,
                        error: None,
                    })?;
                    record_finalizer.disarm();
                }
                drop(download_guard);
                drop(download_permit);
                drop(download_lock_guard);
                self.enforce_audio_cache_limit()?;
                if let Some(cancellation) = cancellation {
                    tokio::select! {
                        () = cancellation.cancelled() => {}
                        () = self.preserve_song_cover_art_for_offline(&client, &song_id) => {}
                    }
                } else {
                    self.preserve_song_cover_art_for_offline(&client, &song_id)
                        .await;
                }
                Ok(Some(DownloadStatus {
                    song_id,
                    cached: true,
                    path: Some(path_to_file_uri(&path)),
                    bytes: byte_count,
                }))
            }
            Some(Err(error)) => {
                let error_message = error.to_string();
                self.record_download(DownloadRecord {
                    entity_type: "song",
                    entity_id: &song_id,
                    song_id: &song_id,
                    status: "failed",
                    path: None,
                    bytes: 0,
                    error: Some(&error_message),
                })?;
                record_finalizer.disarm();
                Err(CoreError::Subsonic(error.to_string()))
            }
        }
    }

    /// # Errors
    /// Returns an error if the cached song or its persisted record cannot be removed.
    pub fn remove_cached_song(&self, song_id: String) -> CoreResult<DownloadStatus> {
        let _cache_guard = cache_mutation_guard()?;
        if self.get_playback_state()?.current_song_id.as_deref() == Some(song_id.as_str()) {
            return Err(CoreError::InvalidInput(format!(
                "song {song_id} is currently playing"
            )));
        }
        if self.song_protected_by_saved_playlist(&song_id)? {
            return Err(CoreError::InvalidInput(format!(
                "song {song_id} is preserved by a saved playlist"
            )));
        }
        if let Some(path) = self.cached_song_path(&song_id)? {
            match std::fs::remove_file(path) {
                Ok(()) => {
                    self.emit_cache_state_event(CacheStateEvent::CachedChanged {
                        song_id: song_id.clone(),
                        cached: false,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.emit_cache_state_event(CacheStateEvent::CachedChanged {
                        song_id: song_id.clone(),
                        cached: false,
                    });
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            self.emit_cache_state_event(CacheStateEvent::CachedChanged {
                song_id: song_id.clone(),
                cached: false,
            });
        }
        let conn = db::open_connection(&self.db_path)?;
        conn.execute("DELETE FROM download_items WHERE song_id = ?1", [&song_id])?;
        Ok(DownloadStatus {
            song_id,
            cached: false,
            path: None,
            bytes: 0,
        })
    }

    /// Removes a cache entry that failed media decoding, even when the song is
    /// normally protected by an offline-saved playlist. A later request may
    /// download a clean replacement.
    ///
    /// # Errors
    /// Returns an error if the cache file or persisted download record cannot be removed.
    pub fn invalidate_cached_song(&self, song_id: &str) -> CoreResult<()> {
        let _cache_guard = cache_mutation_guard()?;
        if let Some(path) = self.cached_song_path(song_id)? {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let conn = db::open_connection(&self.db_path)?;
        conn.execute("DELETE FROM download_items WHERE song_id = ?1", [song_id])?;
        self.emit_cache_state_event(CacheStateEvent::CachedChanged {
            song_id: song_id.to_string(),
            cached: false,
        });
        Ok(())
    }

    /// # Errors
    /// Returns an error if album metadata cannot be read or one of its songs cannot be downloaded.
    pub async fn download_album(&self, album_id: String) -> CoreResult<Vec<DownloadStatus>> {
        let songs = self.get_songs(Some(album_id), None)?;
        let mut statuses = Vec::with_capacity(songs.len());
        for song in songs {
            statuses.push(self.download_song(song.id).await?);
        }
        Ok(statuses)
    }

    /// # Errors
    /// Returns an error if the playlist cannot be refreshed or one of its songs cannot be downloaded.
    pub async fn download_playlist(&self, playlist_id: String) -> CoreResult<Vec<DownloadStatus>> {
        if self.connected_client().await.is_ok() {
            self.refresh_playlist_cache(&playlist_id).await?;
        }
        self.download_local_playlist_songs(&playlist_id).await
    }

    /// # Errors
    /// Returns an error if offline mode prevents saving or playlist cache state cannot be updated.
    pub async fn set_playlist_saved_offline(
        &self,
        playlist_id: String,
        saved_offline: bool,
    ) -> CoreResult<SavedPlaylistOfflineResult> {
        if saved_offline {
            if self.manual_offline_enabled()? {
                return Err(CoreError::OfflineMode);
            }

            let previous_saved_at = self.playlist_offline_saved_at(&playlist_id)?;
            let saved_at = previous_saved_at
                .clone()
                .unwrap_or_else(|| Utc::now().to_rfc3339());
            self.set_playlist_offline_saved_at(&playlist_id, Some(&saved_at))?;
            match self.download_playlist(playlist_id.clone()).await {
                Ok(statuses) => {
                    let downloaded_count = i32::try_from(statuses.len()).map_err(|_| {
                        CoreError::InvalidInput("playlist contains too many songs".to_string())
                    })?;
                    Ok(SavedPlaylistOfflineResult {
                        playlist_id,
                        saved_offline: true,
                        downloaded_count,
                        removed_count: 0,
                        skipped_protected_count: 0,
                    })
                }
                Err(error) => {
                    self.set_playlist_offline_saved_at(&playlist_id, previous_saved_at.as_deref())?;
                    Err(error)
                }
            }
        } else {
            let song_ids = self.playlist_song_ids(&playlist_id)?;
            self.set_playlist_offline_saved_at(&playlist_id, None)?;
            let (removed_count, skipped_protected_count) =
                self.remove_unprotected_cached_songs(song_ids)?;
            Ok(SavedPlaylistOfflineResult {
                playlist_id,
                saved_offline: false,
                downloaded_count: 0,
                removed_count,
                skipped_protected_count,
            })
        }
    }

    /// # Errors
    /// Returns an error if offline mode prevents saving or playlist cache state cannot be updated.
    pub fn mark_playlist_saved_offline(
        &self,
        playlist_id: String,
        saved_offline: bool,
    ) -> CoreResult<SavedPlaylistOfflineResult> {
        if saved_offline {
            if self.manual_offline_enabled()? {
                return Err(CoreError::OfflineMode);
            }

            let previous_saved_at = self.playlist_offline_saved_at(&playlist_id)?;
            let saved_at = previous_saved_at.unwrap_or_else(|| Utc::now().to_rfc3339());
            self.set_playlist_offline_saved_at(&playlist_id, Some(&saved_at))?;
            Ok(SavedPlaylistOfflineResult {
                playlist_id,
                saved_offline: true,
                downloaded_count: 0,
                removed_count: 0,
                skipped_protected_count: 0,
            })
        } else {
            let song_ids = self.playlist_song_ids(&playlist_id)?;
            self.set_playlist_offline_saved_at(&playlist_id, None)?;
            let (removed_count, skipped_protected_count) =
                self.remove_unprotected_cached_songs(song_ids)?;
            Ok(SavedPlaylistOfflineResult {
                playlist_id,
                saved_offline: false,
                downloaded_count: 0,
                removed_count,
                skipped_protected_count,
            })
        }
    }

    /// # Errors
    /// Returns an error if saved playlists cannot be read or their download counts exceed supported limits.
    pub async fn reconcile_saved_playlists_offline(
        &self,
    ) -> CoreResult<Vec<SavedPlaylistOfflineResult>> {
        if self.manual_offline_enabled()? {
            return Ok(Vec::new());
        }

        let playlist_ids = {
            let conn = db::open_connection(&self.db_path)?;
            let mut stmt = conn.prepare(
                "SELECT id FROM playlists WHERE offline_saved_at IS NOT NULL ORDER BY name",
            )?;
            stmt.query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };

        let mut results = Vec::with_capacity(playlist_ids.len());
        for playlist_id in playlist_ids {
            let downloaded_count = match self.download_playlist(playlist_id.clone()).await {
                Ok(statuses) => i32::try_from(statuses.len()).map_err(|_| {
                    CoreError::InvalidInput("playlist contains too many songs".to_string())
                })?,
                Err(error) => {
                    warn!("Failed to reconcile saved playlist {playlist_id}: {error}");
                    0
                }
            };
            results.push(SavedPlaylistOfflineResult {
                playlist_id,
                saved_offline: true,
                downloaded_count,
                removed_count: 0,
                skipped_protected_count: 0,
            });
        }
        Ok(results)
    }

    pub fn get_downloading_song_ids(&self) -> Vec<String> {
        let mut song_ids = DOWNLOADS_IN_PROGRESS
            .lock()
            .map(|downloads| downloads.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        song_ids.sort_unstable();
        song_ids
    }

    /// # Errors
    /// Returns an error if queue, processing settings, or prefetched downloads cannot be accessed.
    pub async fn prefetch_next(&self) -> CoreResult<Vec<DownloadStatus>> {
        let prefetch_count = self
            .get_audio_processing_settings()?
            .prefetch_count
            .to_usize()
            .ok_or_else(|| {
                CoreError::InvalidInput("prefetch_count does not fit usize".to_string())
            })?;
        self.prefetch_upcoming(prefetch_count).await
    }

    /// Builds the bounded set of upcoming queue targets for one prefetch generation.
    /// Cached and currently downloading songs remain in the window so repeated
    /// requests cannot walk progressively farther through the queue.
    ///
    /// # Errors
    /// Returns an error if queue state cannot be read.
    pub fn queue_prefetch_plan(&self, prefetch_count: usize) -> CoreResult<QueuePrefetchPlan> {
        let mut queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
        let queue_revision = self.queue_revision.load(Ordering::Acquire);
        let current_index = queue.current_index();
        if prefetch_count == 0 {
            return Ok(QueuePrefetchPlan {
                queue_revision,
                current_index,
                song_ids: Vec::new(),
            });
        }

        let queue_length = queue.items().len();
        let mut seen = HashSet::new();
        let song_ids = queue
            .peek_upcoming(queue_length)
            .into_iter()
            .filter_map(|item| seen.insert(item.song_id.clone()).then_some(item.song_id))
            .take(prefetch_count)
            .collect();

        Ok(QueuePrefetchPlan {
            queue_revision,
            current_index,
            song_ids,
        })
    }

    /// Returns whether a completed plan still matches the queue generation and
    /// every target remains present in the audio cache.
    ///
    /// # Errors
    /// Returns an error if cache state cannot be inspected.
    pub fn queue_prefetch_plan_is_satisfied(&self, plan: &QueuePrefetchPlan) -> CoreResult<bool> {
        if self.queue_revision.load(Ordering::Acquire) != plan.queue_revision {
            return Ok(false);
        }
        for song_id in &plan.song_ids {
            if self.cached_song_path(song_id)?.is_none() {
                return Ok(false);
            }
        }
        Ok(self.queue_revision.load(Ordering::Acquire) == plan.queue_revision)
    }

    /// # Errors
    /// Returns an error if queue state cannot be read or an upcoming song cannot be downloaded.
    pub async fn prefetch_upcoming(
        &self,
        prefetch_count: usize,
    ) -> CoreResult<Vec<DownloadStatus>> {
        let plan = self.queue_prefetch_plan(prefetch_count)?;
        let mut statuses = Vec::new();
        for song_id in plan.song_ids {
            if self.cached_song_path(&song_id)?.is_some() {
                continue;
            }
            if self
                .get_downloading_song_ids()
                .into_iter()
                .any(|downloading_song_id| downloading_song_id == song_id)
            {
                continue;
            }
            statuses.push(self.download_song(song_id).await?);
        }

        Ok(statuses)
    }

    /// Executes a previously captured queue prefetch generation. Stale or
    /// cancelled generations stop between songs and interrupt in-flight network requests.
    /// Individual failures are retried with bounded backoff and then cooled down
    /// so one unavailable song cannot keep a background worker hot.
    ///
    /// # Errors
    /// Returns an error if cache, queue, offline, or failure state cannot be inspected.
    pub async fn run_queue_prefetch_plan(
        &self,
        plan: &QueuePrefetchPlan,
        cancellation: &PrefetchCancellationToken,
    ) -> CoreResult<QueuePrefetchOutcome> {
        let mut outcome = QueuePrefetchOutcome {
            statuses: Vec::new(),
            completed: true,
        };

        for song_id in &plan.song_ids {
            if cancellation.is_cancelled()
                || self.queue_revision.load(Ordering::Acquire) != plan.queue_revision
                || self.manual_offline_enabled()?
            {
                outcome.completed = false;
                break;
            }

            if self.cached_song_path(song_id)?.is_some() {
                self.clear_prefetch_failure(song_id)?;
                continue;
            }
            if self.prefetch_failure_is_cooling_down(song_id)? {
                outcome.completed = false;
                continue;
            }

            let mut downloaded = false;
            for attempt in 0..PREFETCH_MAX_ATTEMPTS {
                match self
                    .download_song_cancellable(song_id.clone(), Some(cancellation))
                    .await
                {
                    Ok(Some(status)) => {
                        self.clear_prefetch_failure(song_id)?;
                        outcome.statuses.push(status);
                        downloaded = true;
                        break;
                    }
                    Ok(None) | Err(CoreError::OfflineMode) => {
                        outcome.completed = false;
                        return Ok(outcome);
                    }
                    Err(error) if attempt.saturating_add(1) < PREFETCH_MAX_ATTEMPTS => {
                        let shift = u32::try_from(attempt).unwrap_or(u32::MAX).min(8);
                        let delay = PREFETCH_RETRY_BASE_DELAY.saturating_mul(1_u32 << shift);
                        warn!(
                            "Prefetch attempt {} failed for {song_id}: {error}; retrying in {} ms",
                            attempt.saturating_add(1),
                            delay.as_millis()
                        );
                        tokio::select! {
                            () = cancellation.cancelled() => {
                                outcome.completed = false;
                                return Ok(outcome);
                            }
                            () = tokio::time::sleep(delay) => {}
                        }
                    }
                    Err(error) => {
                        self.record_prefetch_failure(song_id)?;
                        warn!(
                            "Prefetch failed for {song_id} after {PREFETCH_MAX_ATTEMPTS} attempts: {error}"
                        );
                        outcome.completed = false;
                        break;
                    }
                }
            }

            if !downloaded {
                continue;
            }
            if cancellation.is_cancelled()
                || self.queue_revision.load(Ordering::Acquire) != plan.queue_revision
            {
                outcome.completed = false;
                break;
            }
        }

        if outcome.completed && !self.queue_prefetch_plan_is_satisfied(plan)? {
            outcome.completed = false;
        }

        Ok(outcome)
    }

    fn prefetch_failure_is_cooling_down(&self, song_id: &str) -> CoreResult<bool> {
        let failures = self
            .prefetch_failures
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?;
        Ok(failures
            .get(song_id)
            .is_some_and(|failure| failure.retry_after > Instant::now()))
    }

    fn record_prefetch_failure(&self, song_id: &str) -> CoreResult<()> {
        let mut failures = self
            .prefetch_failures
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?;
        let consecutive_failures = failures
            .get(song_id)
            .map_or(1, |failure| failure.consecutive_failures.saturating_add(1));
        let shift = consecutive_failures.saturating_sub(1).min(8);
        let cooldown = PREFETCH_FAILURE_COOLDOWN
            .saturating_mul(1_u32 << shift)
            .min(PREFETCH_MAX_FAILURE_COOLDOWN);
        failures.insert(
            song_id.to_string(),
            PrefetchFailureState {
                consecutive_failures,
                retry_after: Instant::now().checked_add(cooldown).ok_or_else(|| {
                    CoreError::InvalidInput("prefetch cooldown exceeds Instant range".to_string())
                })?,
            },
        );
        Ok(())
    }

    fn clear_prefetch_failure(&self, song_id: &str) -> CoreResult<()> {
        self.prefetch_failures
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?
            .remove(song_id);
        Ok(())
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn peek_next_queue_item(&self) -> CoreResult<Option<QueueItem>> {
        let mut queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
        Ok(queue.peek_next().cloned())
    }

    /// Monotonic counter bumped by every queue mutation. Lets callers detect
    /// queue changes without cloning the queue.
    pub fn queue_revision(&self) -> u64 {
        self.queue_revision.load(Ordering::Acquire)
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked.
    pub fn preview_next_queue_item(&self, force: Option<bool>) -> CoreResult<Option<QueueItem>> {
        let queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
        Ok(queue.preview_next(force.unwrap_or(false)))
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked.
    pub fn preview_previous_queue_item(&self) -> CoreResult<Option<QueueItem>> {
        let queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
        Ok(queue.preview_previous())
    }

    /// # Errors
    /// Returns an error if either song's playback metadata cannot be read.
    pub fn songs_are_gapless_eligible(
        &self,
        current_song_id: &str,
        next_song_id: &str,
    ) -> CoreResult<bool> {
        // The playback tick re-evaluates the same transition several times a
        // second; track metadata only changes on library sync or backup
        // import, which invalidate this cache.
        let generation = if let Ok(cache) = self.gapless_eligibility_cache.lock() {
            if let Some(((cached_current, cached_next), eligible)) = cache.value.as_ref()
                && cached_current == current_song_id
                && cached_next == next_song_id
            {
                return Ok(*eligible);
            }
            Some(cache.generation)
        } else {
            None
        };

        let conn = db::open_connection(&self.db_path)?;
        let eligible = match (
            gapless_track_info(&conn, current_song_id)?,
            gapless_track_info(&conn, next_song_id)?,
        ) {
            (Some(current), Some(next)) if current.album_id == next.album_id => {
                let same_disc_consecutive = current.disc_number == next.disc_number
                    && next.track_number == current.track_number.saturating_add(1);
                let next_disc_first_track = next.disc_number
                    == current.disc_number.saturating_add(1)
                    && next.track_number == 1;
                same_disc_consecutive || next_disc_first_track
            }
            _ => false,
        };

        if let Some(generation) = generation
            && let Ok(mut cache) = self.gapless_eligibility_cache.lock()
        {
            cache.store_if_current(
                generation,
                (
                    (current_song_id.to_string(), next_song_id.to_string()),
                    eligible,
                ),
            );
        }
        Ok(eligible)
    }

    fn invalidate_gapless_eligibility_cache(&self) {
        if let Ok(mut cache) = self.gapless_eligibility_cache.lock() {
            cache.invalidate();
        }
    }

    /// # Errors
    /// Returns an error if playback state cannot be read from the database.
    pub fn get_playback_state(&self) -> CoreResult<PlaybackState> {
        let conn = db::open_connection(&self.db_path)?;
        let state = conn
            .query_row(
                "SELECT current_song_id, position_seconds, duration_seconds, was_playing,
                        app_volume, updated_at
                 FROM playback_state WHERE id = 1",
                [],
                |row| {
                    Ok(PlaybackState {
                        current_song_id: row.get(0)?,
                        position_seconds: row.get(1)?,
                        duration_seconds: row.get(2)?,
                        was_playing: row.get::<_, i64>(3)? != 0,
                        app_volume: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()?;

        Ok(state.unwrap_or_else(|| PlaybackState {
            current_song_id: None,
            position_seconds: 0.0,
            duration_seconds: 0.0,
            was_playing: false,
            app_volume: 1.0,
            updated_at: Utc::now().to_rfc3339(),
        }))
    }

    /// # Errors
    /// Returns an error if playback or scrobble state cannot be read, updated, or persisted.
    pub async fn report_playback_progress(
        &self,
        progress: PlaybackProgress,
    ) -> CoreResult<PlaybackState> {
        let previous = self.playback_markers()?;
        let lastfm_track = lastfm::track_for_song(&self.db_path, &progress.song_id)?;
        let manual_offline_enabled = self.manual_offline_enabled()?;
        let now_playing_song_id =
            if previous.now_playing_song_id.as_deref() == Some(&progress.song_id) {
                previous.now_playing_song_id.clone()
            } else {
                if let Ok(client) = self.connected_client().await {
                    let _ = client
                        .scrobble(vec![(progress.song_id.clone(), None)], Some(false))
                        .await;
                }
                if let Some(track) = &lastfm_track
                    && !manual_offline_enabled
                {
                    let _ = lastfm::report_now_playing(&self.db_path, track).await;
                }
                Some(progress.song_id.clone())
            };

        let should_submit = progress.duration_seconds > 0.0
            && progress.position_seconds / progress.duration_seconds >= 0.5
            && previous.scrobbled_song_id.as_deref() != Some(&progress.song_id);
        if let Some(track) = lastfm_track.clone() {
            let inserted = lastfm::maybe_enqueue_from_progress(
                &self.db_path,
                track,
                progress.position_seconds,
                progress.duration_seconds,
            )?;
            if inserted && !manual_offline_enabled {
                let _ = self.retry_lastfm_queue_inner(false).await;
            }
        }

        let scrobbled_song_id = if should_submit {
            if let Ok(client) = self.connected_client().await
                && let Ok(timestamp) = usize::try_from(chrono::Utc::now().timestamp_millis().max(0))
            {
                let _ = client
                    .scrobble(
                        vec![(progress.song_id.clone(), Some(timestamp))],
                        Some(true),
                    )
                    .await;
            }
            Some(progress.song_id.clone())
        } else if previous.now_playing_song_id.as_deref() == Some(&progress.song_id) {
            previous.scrobbled_song_id
        } else {
            None
        };

        self.save_playback_state(&PlaybackStateWrite {
            song_id: Some(progress.song_id),
            position_seconds: progress.position_seconds,
            duration_seconds: progress.duration_seconds,
            was_playing: progress.is_playing,
            app_volume: previous.app_volume,
            now_playing_song_id,
            scrobbled_song_id,
        })
    }

    /// # Errors
    /// Returns an error if playback markers cannot be read or updated state cannot be persisted.
    pub fn save_playback_position(&self, progress: PlaybackProgress) -> CoreResult<PlaybackState> {
        let previous = self.playback_markers()?;
        self.save_playback_state(&PlaybackStateWrite {
            song_id: Some(progress.song_id),
            position_seconds: progress.position_seconds,
            duration_seconds: progress.duration_seconds,
            was_playing: progress.is_playing,
            app_volume: previous.app_volume,
            now_playing_song_id: previous.now_playing_song_id,
            scrobbled_song_id: previous.scrobbled_song_id,
        })
    }

    pub fn get_lastfm_status(&self) -> LastfmStatus {
        lastfm::status(&self.db_path)
    }

    /// # Errors
    /// Returns an error if offline mode is active or Last.fm authentication cannot be started.
    pub async fn begin_lastfm_auth(&self) -> CoreResult<LastfmAuthStart> {
        if self.manual_offline_enabled()? {
            return Err(CoreError::OfflineMode);
        }

        lastfm::begin_auth(&self.db_path).await
    }

    /// # Errors
    /// Returns an error if offline mode is active or Last.fm authentication cannot be completed.
    pub async fn complete_lastfm_auth(&self) -> CoreResult<LastfmStatus> {
        if self.manual_offline_enabled()? {
            return Err(CoreError::OfflineMode);
        }

        let _ = lastfm::complete_auth(&self.db_path).await?;
        let _ = self.retry_lastfm_queue_inner(true).await;
        Ok(lastfm::status(&self.db_path))
    }

    /// # Errors
    /// Returns an error if the Last.fm session cannot be removed from persisted state.
    pub fn disconnect_lastfm(&self) -> CoreResult<LastfmStatus> {
        lastfm::disconnect(&self.db_path)
    }

    /// # Errors
    /// Returns an error if the persisted Last.fm queue cannot be read.
    pub fn get_lastfm_queue(&self) -> CoreResult<Vec<LastfmQueueItem>> {
        lastfm::list_queue(&self.db_path)
    }

    /// # Errors
    /// Returns an error if offline mode is active or queued Last.fm submissions cannot be retried.
    pub async fn retry_lastfm_queue(&self) -> CoreResult<usize> {
        if self.manual_offline_enabled()? {
            return Err(CoreError::OfflineMode);
        }

        self.retry_lastfm_queue_inner(true).await
    }

    /// # Errors
    /// Returns an error if audio processing settings cannot be read from the database.
    pub fn get_audio_processing_settings(&self) -> CoreResult<AudioProcessingSettings> {
        // The playback tick path reads these settings several times a second,
        // so they are memoized until a setter or backup import replaces them.
        if let Ok(cache) = self.audio_processing_settings_cache.lock()
            && let Some(settings) = cache.as_ref()
        {
            return Ok(settings.clone());
        }
        let conn = db::open_connection(&self.db_path)?;
        let mut settings = match sync_value(&conn, "settings_audio_processing")? {
            Some(json) => {
                serde_json::from_str::<AudioProcessingSettings>(&json).unwrap_or_default()
            }
            None => AudioProcessingSettings::default(),
        };
        clamp_audio_processing_settings(&mut settings);
        if let Ok(mut cache) = self.audio_processing_settings_cache.lock() {
            *cache = Some(settings.clone());
        }
        Ok(settings)
    }

    fn invalidate_audio_processing_settings_cache(&self) {
        if let Ok(mut cache) = self.audio_processing_settings_cache.lock() {
            *cache = None;
        }
    }

    /// # Errors
    /// Returns an error if audio processing settings cannot be serialized or persisted.
    pub fn set_audio_processing_settings(
        &self,
        mut settings: AudioProcessingSettings,
    ) -> CoreResult<AudioProcessingSettings> {
        clamp_audio_processing_settings(&mut settings);
        let conn = db::open_connection(&self.db_path)?;
        write_sync_value(
            &conn,
            "settings_audio_processing",
            &serde_json::to_string(&settings)?,
        )?;
        if let Ok(mut cache) = self.audio_processing_settings_cache.lock() {
            *cache = Some(settings.clone());
        }
        Ok(settings)
    }

    /// Writes a portable metadata backup without credentials or cached media.
    ///
    /// # Errors
    /// Returns an error if persisted data cannot be read, validated, or written.
    pub fn export_portable_backup(&self, path: impl AsRef<Path>) -> CoreResult<BackupSummary> {
        let _queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
        let mut conn = db::open_connection(&self.db_path)?;
        let backup = backup::export_from_connection(
            &mut conn,
            PortablePreferences {
                sync: Some(self.get_sync_settings()?),
                connectivity: Some(self.get_connectivity_settings()?),
                audio_processing: Some(self.get_audio_processing_settings()?),
                volume: Some(self.get_playback_state()?.app_volume),
            },
        )?;
        backup::write_to_file(path.as_ref(), &backup)?;
        Ok(backup.summary())
    }

    /// Replaces portable metadata with a validated backup while retaining local credentials and caches.
    ///
    /// # Errors
    /// Returns an error if the backup is invalid or persisted state cannot be replaced.
    pub fn import_portable_backup(&self, path: impl AsRef<Path>) -> CoreResult<BackupSummary> {
        let backup = backup::read_from_file(path.as_ref())?;
        let mut queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
        let mut conn = db::open_connection(&self.db_path)?;
        let summary = backup::import_into_connection(&mut conn, &backup)?;
        *queue = PlayQueue::load_with_original_order(
            backup.queue.items.clone(),
            backup.queue.original_items.clone(),
            backup.queue.current_index,
            backup.queue.shuffle,
            backup.queue.repeat_mode,
        );
        self.queue_revision.fetch_add(1, Ordering::AcqRel);
        // The imported backup may carry audio processing preferences and
        // replace song metadata wholesale.
        self.invalidate_audio_processing_settings_cache();
        self.invalidate_gapless_eligibility_cache();
        self.emit_cache_state_event(CacheStateEvent::Reconcile);
        Ok(summary)
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked.
    pub fn get_queue(&self) -> CoreResult<QueueState> {
        let mut queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
        // Reads only touch the in-memory queue; prepare_next_cycle_if_needed
        // inside from_queue mutates prepared_shuffle_cycle, which is never
        // persisted, so skipping the save keeps reads off the write path.
        Ok(QueueState::from_queue(&mut queue))
    }

    // Owned collection arguments are retained as part of the stable FFI-facing API.
    /// # Errors
    /// Returns an error if the requested songs are invalid or queue state cannot be loaded and persisted.
    #[allow(clippy::needless_pass_by_value)]
    pub fn play_song_with_queue(
        &self,
        song_id: String,
        song_ids: Vec<String>,
    ) -> CoreResult<QueueState> {
        if song_ids.is_empty() {
            return Err(CoreError::InvalidInput(
                "Cannot play from an empty queue".to_string(),
            ));
        }

        let queue_items = self.load_queue_items_for_song_ids(&song_ids)?;
        let current_index = queue_items
            .iter()
            .position(|item| item.song_id == song_id)
            .ok_or_else(|| CoreError::InvalidInput("Selected song is not available".to_string()))?;

        self.with_queue_mutation_state(|queue| {
            *queue = PlayQueue::load(queue_items, Some(current_index), false, RepeatMode::Off);
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn add_to_queue(&self, item: QueueItem) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.add(item);
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn add_songs_to_queue(&self, items: Vec<QueueItem>) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.add_many(items);
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn insert_next(&self, item: QueueItem) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.insert_next(item);
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn insert_next_songs(&self, items: Vec<QueueItem>) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.insert_many_next(items);
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn remove_from_queue(&self, index: usize) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.remove(index);
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn clear_queue(&self) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.clear();
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn move_queue_item(&self, from: usize, to: usize) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.move_item(from, to);
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn play_queue_item(&self, index: usize) -> CoreResult<Option<QueueItem>> {
        self.with_queue_mutation_result(|queue| Ok(queue.set_current(index).cloned()))
            .map(|(item, _)| item)
    }

    /// Selects a queue item only if the current and target songs still match
    /// the state observed before asynchronous playback preparation.
    ///
    /// # Errors
    /// Returns an error if the queue changed, cannot be locked, or cannot be persisted.
    pub fn play_queue_item_if_matches(
        &self,
        index: usize,
        expected_current_song_id: Option<&str>,
        expected_target_song_id: &str,
    ) -> CoreResult<Option<QueueItem>> {
        self.with_queue_mutation_result(|queue| {
            ensure_queue_navigation_matches(
                queue,
                expected_current_song_id,
                queue.items().get(index),
                expected_target_song_id,
            )?;
            Ok(queue.set_current(index).cloned())
        })
        .map(|(item, _)| item)
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn play_next(&self, force: Option<bool>) -> CoreResult<Option<QueueItem>> {
        self.with_queue_mutation_result(|queue| Ok(queue.next(force.unwrap_or(false)).cloned()))
            .map(|(item, _)| item)
    }

    /// Advances only if queue navigation still resolves to the prepared target.
    ///
    /// # Errors
    /// Returns an error if the queue changed, cannot be locked, or cannot be persisted.
    pub fn play_next_if_matches(
        &self,
        force: Option<bool>,
        expected_current_song_id: Option<&str>,
        expected_target_song_id: &str,
    ) -> CoreResult<Option<QueueItem>> {
        self.with_queue_mutation_result(|queue| {
            let target = queue.preview_next(force.unwrap_or(false));
            ensure_queue_navigation_matches(
                queue,
                expected_current_song_id,
                target.as_ref(),
                expected_target_song_id,
            )?;
            Ok(queue.next(force.unwrap_or(false)).cloned())
        })
        .map(|(item, _)| item)
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn play_previous(&self) -> CoreResult<Option<QueueItem>> {
        self.with_queue_mutation_result(|queue| Ok(queue.previous().cloned()))
            .map(|(item, _)| item)
    }

    /// Moves backward only if queue navigation still resolves to the prepared target.
    ///
    /// # Errors
    /// Returns an error if the queue changed, cannot be locked, or cannot be persisted.
    pub fn play_previous_if_matches(
        &self,
        expected_current_song_id: Option<&str>,
        expected_target_song_id: &str,
    ) -> CoreResult<Option<QueueItem>> {
        self.with_queue_mutation_result(|queue| {
            let target = queue.preview_previous();
            ensure_queue_navigation_matches(
                queue,
                expected_current_song_id,
                target.as_ref(),
                expected_target_song_id,
            )?;
            Ok(queue.previous().cloned())
        })
        .map(|(item, _)| item)
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn toggle_shuffle(&self) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.toggle_shuffle();
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn set_repeat_mode(&self, mode: RepeatMode) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.set_repeat_mode(mode);
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn cycle_repeat_mode(&self) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.cycle_repeat_mode();
            Ok(())
        })
    }

    /// # Errors
    /// Returns an error if queue state cannot be locked or persisted.
    pub fn reroll_next(&self) -> CoreResult<QueueState> {
        self.with_queue_mutation_state(|queue| {
            queue.reroll_next();
            Ok(())
        })
    }

    async fn connected_client(&self) -> CoreResult<Client> {
        if self.manual_offline_enabled()? {
            return Err(CoreError::OfflineMode);
        }

        self.client
            .lock()
            .await
            .clone()
            .ok_or(CoreError::NotConnected)
    }

    fn current_config(&self) -> CoreResult<ServerConfig> {
        self.server_config
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?
            .clone()
            .ok_or(CoreError::NotConnected)
    }

    fn get_cached_playlist(&self, playlist_id: &str) -> CoreResult<Playlist> {
        let conn = db::open_connection(&self.db_path)?;
        conn.query_row(
            "SELECT id, name, song_count, duration, owner, cover_art_id,
                    created_at, changed_at, offline_saved_at
             FROM playlists
             WHERE id = ?1",
            [playlist_id],
            playlist_from_row,
        )
        .map_err(Into::into)
    }

    fn get_cached_playlists(&self, saved_offline_only: bool) -> CoreResult<Vec<Playlist>> {
        let conn = db::open_connection(&self.db_path)?;
        let sql = if saved_offline_only {
            "SELECT p.id, p.name, p.song_count, p.duration, p.owner, p.cover_art_id,
                    p.created_at, p.changed_at, p.offline_saved_at
             FROM playlists p
             WHERE p.offline_saved_at IS NOT NULL
             ORDER BY p.name COLLATE NOCASE"
        } else {
            "SELECT p.id, p.name, p.song_count, p.duration, p.owner, p.cover_art_id,
                    p.created_at, p.changed_at, p.offline_saved_at
             FROM playlists p
             ORDER BY p.name COLLATE NOCASE"
        };
        let mut stmt = conn.prepare(sql)?;
        rows_collect(stmt.query_map([], playlist_from_row)?)
    }

    fn get_local_playlist_songs(&self, playlist_id: &str) -> CoreResult<Vec<Song>> {
        let conn = db::open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT s.id, s.album_id, s.artist_id, s.title, s.track_number, s.disc_number,
                    s.duration, s.bit_rate, s.size, s.suffix, s.content_type, s.path,
                    s.year, s.genre, s.synced_at, ar.name, al.name
             FROM playlist_songs ps
             JOIN songs s ON ps.song_id = s.id
             LEFT JOIN artists ar ON s.artist_id = ar.id
             LEFT JOIN albums al ON s.album_id = al.id
             WHERE ps.playlist_id = ?1
             ORDER BY ps.position",
        )?;
        rows_collect(stmt.query_map([playlist_id], Song::from_row)?)
    }

    fn save_playlists(&self, playlists: &[Playlist]) -> CoreResult<()> {
        let mut conn = db::open_connection(&self.db_path)?;
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO playlists
                 (id, name, song_count, duration, owner, cover_art_id, created_at, changed_at, offline_saved_at, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, (SELECT offline_saved_at FROM playlists WHERE id = ?1), ?9)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    song_count = excluded.song_count,
                    duration = excluded.duration,
                    owner = excluded.owner,
                    cover_art_id = excluded.cover_art_id,
                    created_at = excluded.created_at,
                    changed_at = excluded.changed_at,
                    synced_at = excluded.synced_at",
            )?;
            for playlist in playlists {
                stmt.execute(params![
                    playlist.id,
                    playlist.name,
                    playlist.song_count,
                    playlist.duration,
                    playlist.owner,
                    playlist.cover_art_id,
                    playlist.created_at,
                    playlist.changed_at,
                    now
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn save_playlist_songs(&self, playlist_id: &str, songs: &[Song]) -> CoreResult<()> {
        let mut conn = db::open_connection(&self.db_path)?;
        let tx = conn.transaction()?;
        let existing_song_ids = {
            let mut stmt = tx.prepare("SELECT id FROM songs")?;
            stmt.query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<HashSet<_>, _>>()?
        };
        tx.execute(
            "DELETE FROM playlist_songs WHERE playlist_id = ?1",
            [playlist_id],
        )?;
        {
            let mut playlist_song_stmt = tx.prepare(
                "INSERT INTO playlist_songs (playlist_id, song_id, position)
                 VALUES (?1, ?2, ?3)",
            )?;
            for (position, song) in songs.iter().enumerate() {
                if existing_song_ids.contains(&song.id) {
                    let position = i64::try_from(position).map_err(|_| {
                        CoreError::InvalidInput(
                            "playlist position exceeds SQLite range".to_string(),
                        )
                    })?;
                    playlist_song_stmt.execute(params![playlist_id, song.id, position])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    async fn refresh_playlist_cache(&self, playlist_id: &str) -> CoreResult<()> {
        let client = self.connected_client().await?;
        let playlist = client
            .get_playlist(playlist_id)
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        let mapped = playlist_from_subsonic(playlist.base);
        let now = Utc::now().to_rfc3339();
        let songs = playlist
            .entry
            .into_iter()
            .map(|song| Song {
                id: song.id,
                album_id: song.album_id.unwrap_or_default(),
                artist_id: song.artist_id.unwrap_or_default(),
                title: song.title,
                track_number: song.track,
                disc_number: song.disc_number.unwrap_or(1),
                duration: song.duration,
                bit_rate: song.bit_rate,
                size: song.size,
                suffix: song.suffix,
                content_type: song.content_type,
                path: song.path,
                year: song.year,
                genre: song.genre,
                synced_at: now.clone(),
                artist: song.artist,
                album: song.album,
            })
            .collect::<Vec<_>>();
        self.save_playlists(&[mapped])?;
        self.save_playlist_songs(playlist_id, &songs)?;
        Ok(())
    }

    fn playlist_saved_offline(&self, playlist_id: &str) -> CoreResult<bool> {
        Ok(self.playlist_offline_saved_at(playlist_id)?.is_some())
    }

    fn playlist_offline_saved_at(&self, playlist_id: &str) -> CoreResult<Option<String>> {
        let conn = db::open_connection(&self.db_path)?;
        conn.query_row(
            "SELECT offline_saved_at FROM playlists WHERE id = ?1",
            [playlist_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map(std::option::Option::flatten)
        .map_err(Into::into)
    }

    fn set_playlist_offline_saved_at(
        &self,
        playlist_id: &str,
        offline_saved_at: Option<&str>,
    ) -> CoreResult<()> {
        let conn = db::open_connection(&self.db_path)?;
        conn.execute(
            "UPDATE playlists SET offline_saved_at = ?1 WHERE id = ?2",
            params![offline_saved_at, playlist_id],
        )?;
        Ok(())
    }

    fn playlist_song_ids(&self, playlist_id: &str) -> CoreResult<HashSet<String>> {
        let conn = db::open_connection(&self.db_path)?;
        let mut stmt = conn.prepare("SELECT song_id FROM playlist_songs WHERE playlist_id = ?1")?;
        stmt.query_map([playlist_id], |row| row.get::<_, String>(0))?
            .collect::<Result<HashSet<_>, _>>()
            .map_err(Into::into)
    }

    pub(crate) fn saved_playlist_song_ids(&self) -> CoreResult<HashSet<String>> {
        let conn = db::open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT ps.song_id
             FROM playlist_songs ps
             JOIN playlists p ON p.id = ps.playlist_id
             WHERE p.offline_saved_at IS NOT NULL",
        )?;
        stmt.query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<HashSet<_>, _>>()
            .map_err(Into::into)
    }

    fn song_cover_art_id(&self, song_id: &str) -> CoreResult<Option<String>> {
        let conn = db::open_connection(&self.db_path)?;
        conn.query_row(
            "SELECT al.cover_art_id
             FROM songs s
             LEFT JOIN albums al ON s.album_id = al.id
             WHERE s.id = ?1",
            [song_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map(std::option::Option::flatten)
        .map_err(Into::into)
    }

    fn playlist_offline_cover_art_ids(&self, playlist_id: &str) -> CoreResult<Vec<String>> {
        let conn = db::open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT cover_art_id FROM (
                SELECT 0 AS sort_order, 0 AS position, p.cover_art_id
                FROM playlists p
                WHERE p.id = ?1
                UNION ALL
                SELECT 1 AS sort_order, ps.position, al.cover_art_id
                FROM playlist_songs ps
                JOIN songs s ON s.id = ps.song_id
                LEFT JOIN albums al ON al.id = s.album_id
                WHERE ps.playlist_id = ?1
                UNION ALL
                SELECT 2 AS sort_order, ps.position, ar.cover_art_id
                FROM playlist_songs ps
                JOIN songs s ON s.id = ps.song_id
                LEFT JOIN artists ar ON ar.id = s.artist_id
                WHERE ps.playlist_id = ?1
            )
            ORDER BY sort_order, position",
        )?;
        let ids = stmt
            .query_map([playlist_id], |row| row.get::<_, Option<String>>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(distinct_nonempty_cover_art_ids(ids))
    }

    async fn download_local_playlist_songs(
        &self,
        playlist_id: &str,
    ) -> CoreResult<Vec<DownloadStatus>> {
        let song_ids = {
            let conn = db::open_connection(&self.db_path)?;
            let mut stmt = conn.prepare(
                "SELECT song_id FROM playlist_songs WHERE playlist_id = ?1 ORDER BY position",
            )?;
            stmt.query_map([playlist_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };

        let mut statuses = Vec::with_capacity(song_ids.len());
        for song_id in song_ids {
            statuses.push(self.download_song(song_id).await?);
        }
        self.preserve_playlist_cover_art_for_offline(playlist_id)
            .await;
        Ok(statuses)
    }

    async fn preserve_cached_song_cover_art_if_connected(&self, song_id: &str) {
        if self.manual_offline_enabled().unwrap_or(true) {
            return;
        }

        let client = self.client.lock().await.clone();
        if let Some(client) = client {
            self.preserve_song_cover_art_for_offline(&client, song_id)
                .await;
        }
    }

    async fn preserve_song_cover_art_for_offline(&self, client: &Client, song_id: &str) {
        let cover_art_id = match self.song_cover_art_id(song_id) {
            Ok(Some(cover_art_id)) => cover_art_id,
            Ok(None) => return,
            Err(error) => {
                warn!("Failed to resolve cover art for offline song {song_id}: {error}");
                return;
            }
        };

        if let Err(error) = self
            .preserve_cover_art_for_offline(client, &cover_art_id)
            .await
        {
            warn!(
                "Failed to preserve cover art {cover_art_id} for offline song {song_id}: {error}"
            );
        }
    }

    async fn preserve_playlist_cover_art_for_offline(&self, playlist_id: &str) {
        if self.manual_offline_enabled().unwrap_or(true) {
            return;
        }

        let client = self.client.lock().await.clone();
        let Some(client) = client else {
            return;
        };

        let cover_art_ids = match self.playlist_offline_cover_art_ids(playlist_id) {
            Ok(cover_art_ids) => cover_art_ids,
            Err(error) => {
                warn!("Failed to resolve cover art for offline playlist {playlist_id}: {error}");
                return;
            }
        };

        for cover_art_id in cover_art_ids {
            if let Err(error) = self
                .preserve_cover_art_for_offline(&client, &cover_art_id)
                .await
            {
                warn!(
                    "Failed to preserve cover art {cover_art_id} for offline playlist {playlist_id}: {error}"
                );
            }
        }
    }

    fn remove_unprotected_cached_songs(&self, song_ids: HashSet<String>) -> CoreResult<(i32, i32)> {
        let _cache_guard = cache_mutation_guard()?;
        let mut removed_count = 0_i32;
        let mut skipped_protected_count = 0_i32;

        for song_id in song_ids {
            if self.song_protected_by_saved_playlist(&song_id)? {
                skipped_protected_count = skipped_protected_count.saturating_add(1);
                continue;
            }
            if let Some(path) = self.cached_song_path(&song_id)? {
                match std::fs::remove_file(&path) {
                    Ok(()) => {
                        removed_count = removed_count.saturating_add(1);
                        self.emit_cache_state_event(CacheStateEvent::CachedChanged {
                            song_id: song_id.clone(),
                            cached: false,
                        });
                        let conn = db::open_connection(&self.db_path)?;
                        conn.execute("DELETE FROM download_items WHERE song_id = ?1", [&song_id])?;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        self.emit_cache_state_event(CacheStateEvent::CachedChanged {
                            song_id: song_id.clone(),
                            cached: false,
                        });
                    }
                    Err(error) => return Err(error.into()),
                }
            } else {
                self.emit_cache_state_event(CacheStateEvent::CachedChanged {
                    song_id,
                    cached: false,
                });
            }
        }

        Ok((removed_count, skipped_protected_count))
    }

    fn sync_job_status(
        conn: &Connection,
        prefix: &str,
        enabled: bool,
        interval_minutes: u32,
    ) -> CoreResult<SyncJobStatus> {
        let last_attempt_at = sync_value(conn, &format!("{prefix}_last_attempt_at"))?;
        let last_success_at = sync_value(conn, &format!("{prefix}_last_success_at"))?;
        let last_error =
            sync_value(conn, &format!("{prefix}_last_error"))?.filter(|e| !e.is_empty());
        let next_run_at = compute_next_run_at(
            enabled,
            interval_minutes,
            last_attempt_at.as_deref(),
            Utc::now(),
        );

        Ok(SyncJobStatus {
            enabled,
            interval_minutes,
            running: false,
            last_attempt_at,
            last_success_at,
            last_error,
            next_run_at,
        })
    }

    fn record_sync_attempt(&self, prefix: &str, error: Option<String>) -> CoreResult<()> {
        let conn = db::open_connection(&self.db_path)?;
        let now = Utc::now().to_rfc3339();
        write_sync_value(&conn, &format!("{prefix}_last_attempt_at"), &now)?;
        if let Some(error) = error {
            write_sync_value(&conn, &format!("{prefix}_last_error"), &error)?;
        }
        Ok(())
    }

    fn record_sync_attempt_keyed(
        &self,
        attempt_key: &str,
        error_key: &str,
        error: Option<String>,
    ) -> CoreResult<()> {
        let conn = db::open_connection(&self.db_path)?;
        let now = Utc::now().to_rfc3339();
        write_sync_value(&conn, attempt_key, &now)?;
        match error {
            Some(error) => write_sync_value(&conn, error_key, &error)?,
            None => write_sync_value(&conn, error_key, "")?,
        }
        Ok(())
    }

    fn record_sync_success_keyed(&self, success_key: &str, error_key: &str) -> CoreResult<()> {
        let conn = db::open_connection(&self.db_path)?;
        let now = Utc::now().to_rfc3339();
        write_sync_value(&conn, success_key, &now)?;
        write_sync_value(&conn, error_key, "")?;
        Ok(())
    }

    fn next_due_sync_job(&self, settings: &SyncSettings) -> CoreResult<Option<DueSyncJob>> {
        let now = Utc::now();
        let conn = db::open_connection(&self.db_path)?;
        let full_last_attempt = sync_value(&conn, FULL_LAST_ATTEMPT_AT_KEY)?;
        let incremental_last_attempt = sync_value(&conn, INCREMENTAL_LAST_ATTEMPT_AT_KEY)?;

        if is_job_due(
            settings.full_reconcile_enabled,
            settings.full_reconcile_interval_hours.saturating_mul(60),
            full_last_attempt.as_deref(),
            now,
        ) {
            return Ok(Some(DueSyncJob::FullReconcile));
        }

        if is_job_due(
            settings.incremental_enabled,
            settings.incremental_interval_minutes,
            incremental_last_attempt.as_deref(),
            now,
        ) {
            return Ok(Some(DueSyncJob::Incremental));
        }

        Ok(None)
    }

    async fn run_incremental_library_sync(&self) -> CoreResult<SyncResult> {
        let client = self.connected_client().await?;
        let (previous_head_album_id, local_artists, local_album_ids) = {
            let conn = db::open_connection(&self.db_path)?;
            (
                sync_value(&conn, NEWEST_HEAD_ALBUM_KEY)?,
                load_local_artists(&conn)?,
                load_local_album_ids(&conn)?,
            )
        };

        let newest_scan = fetch_newest_album_candidates(
            &client,
            previous_head_album_id.as_deref(),
            &local_album_ids,
        )
        .await?;

        if newest_scan.candidates.is_empty() {
            handle_empty_incremental_scan(
                &self.db_path,
                previous_head_album_id.as_deref(),
                &newest_scan,
            )?;
            return Ok(SyncResult::default());
        }

        let candidate_count = newest_scan.candidates.len();
        let sync_data =
            fetch_incremental_library_data(client, &newest_scan.candidates, &local_artists).await?;

        info!(
            "Applying newest-album incremental sync: importing {} newest albums via {} artists (upserts: {} artists, {} albums, {} songs)",
            candidate_count,
            sync_data.artists.len(),
            sync_data.artists.len(),
            sync_data.albums.len(),
            sync_data.songs.len()
        );

        let now = Utc::now().to_rfc3339();
        let mut conn = db::open_connection(&self.db_path)?;
        let tx = conn.transaction()?;
        apply_library_sync_data(
            &tx,
            &sync_data.artists,
            &sync_data.albums,
            &sync_data.songs,
            &now,
        )?;
        if let Some(head_album_id) = newest_scan.head_album_id.as_deref() {
            tx.execute(
                "INSERT OR REPLACE INTO sync_state (key, value, updated_at)
                 VALUES (?1, ?2, ?3)",
                params![NEWEST_HEAD_ALBUM_KEY, head_album_id, &now],
            )?;
        }
        tx.commit()?;
        self.invalidate_gapless_eligibility_cache();

        Ok(SyncResult {
            artists: sync_data.artists.len(),
            albums: sync_data.albums.len(),
            songs: sync_data.songs.len(),
        })
    }

    fn load_queue_items_for_song_ids(&self, song_ids: &[String]) -> CoreResult<Vec<QueueItem>> {
        if song_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = vec!["?"; song_ids.len()].join(", ");
        let query = format!(
            "SELECT s.id, s.title, a.name, al.name, s.duration
             FROM songs s
             LEFT JOIN artists a ON s.artist_id = a.id
             LEFT JOIN albums al ON s.album_id = al.id
             WHERE s.id IN ({placeholders})"
        );

        let conn = db::open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(&query)?;
        let items_by_id = stmt
            .query_map(params_from_iter(song_ids.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    QueueItem {
                        song_id: row.get(0)?,
                        title: row.get(1)?,
                        artist: row
                            .get::<_, Option<String>>(2)?
                            .unwrap_or_else(|| "Unknown Artist".to_string()),
                        album: row
                            .get::<_, Option<String>>(3)?
                            .unwrap_or_else(|| "Unknown Album".to_string()),
                        duration: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
                    },
                ))
            })?
            .collect::<Result<HashMap<_, _>, _>>()?;

        Ok(song_ids
            .iter()
            .filter_map(|song_id| items_by_id.get(song_id).cloned())
            .collect())
    }

    fn audio_cache_dir(&self) -> CoreResult<PathBuf> {
        let path = self.data_dir.join("audio_cache");
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn audio_cache_path(&self, song_id: &str, suffix: &str) -> CoreResult<PathBuf> {
        let safe_id = sanitize_file_component(song_id);
        let filename = if suffix.is_empty() {
            safe_id
        } else {
            format!("{safe_id}.{suffix}")
        };
        Ok(self.audio_cache_dir()?.join(filename))
    }

    fn cover_cache_dir(&self) -> CoreResult<PathBuf> {
        let path = self.data_dir.join("cover_cache");
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn cover_cache_path(&self, cover_art_id: &str, size: Option<i32>) -> CoreResult<PathBuf> {
        Ok(self
            .cover_cache_dir()?
            .join(cover_cache_filename(cover_art_id, size)))
    }

    fn fallback_cover_cache_paths(
        &self,
        cover_art_id: &str,
        size: Option<i32>,
    ) -> CoreResult<Vec<PathBuf>> {
        let mut sizes = Vec::new();
        push_unique_cover_art_size(&mut sizes, size);
        push_unique_cover_art_size(&mut sizes, Some(LARGE_COVER_ART_SIZE));
        push_unique_cover_art_size(&mut sizes, Some(128));
        push_unique_cover_art_size(&mut sizes, None);

        let cache_dir = self.cover_cache_dir()?;
        Ok(sizes
            .into_iter()
            .map(|candidate_size| {
                cache_dir.join(cover_cache_filename(cover_art_id, candidate_size))
            })
            .collect())
    }

    fn cached_cover_art_path(
        &self,
        cover_art_id: &str,
        size: Option<i32>,
    ) -> CoreResult<Option<PathBuf>> {
        let fallback_paths = self.fallback_cover_cache_paths(cover_art_id, size)?;
        for path in &fallback_paths {
            if path.exists() {
                return Ok(Some(path.clone()));
            }
        }

        let known_paths = fallback_paths.into_iter().collect::<HashSet<_>>();
        let sanitized_id = sanitize_file_component(cover_art_id);
        let mut discovered_paths = std::fs::read_dir(self.cover_cache_dir()?)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .filter(|path| !known_paths.contains(path))
            .filter(|path| {
                path.file_name()
                    .and_then(|file_name| file_name.to_str())
                    .is_some_and(|file_name| cover_art_filename_matches(file_name, &sanitized_id))
            })
            .collect::<Vec<_>>();
        discovered_paths.sort();
        Ok(discovered_paths.into_iter().next())
    }

    async fn get_or_cache_cover_art(
        &self,
        cover_art_id: &str,
        size: Option<i32>,
    ) -> CoreResult<PathBuf> {
        if let Some(path) = self.cached_cover_art_path(cover_art_id, size)? {
            return Ok(path);
        }

        let path = self.cover_cache_path(cover_art_id, size)?;
        let client = self.connected_client().await?;
        let bytes = client
            .get_cover_art(cover_art_id, size)
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, bytes)?;
        Ok(path)
    }

    async fn preserve_cover_art_for_offline(
        &self,
        client: &Client,
        cover_art_id: &str,
    ) -> CoreResult<()> {
        if cover_art_id.trim().is_empty() {
            return Ok(());
        }

        let path = self.cover_cache_path(cover_art_id, Some(LARGE_COVER_ART_SIZE))?;
        if path.exists() {
            return Ok(());
        }

        let bytes = client
            .get_cover_art(cover_art_id, Some(LARGE_COVER_ART_SIZE))
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)?;
        Ok(())
    }

    fn prefetch_large_cover_art_if_small(&self, cover_art_id: &str, size: Option<i32>) {
        if !should_prefetch_large_cover_art(size) {
            return;
        }

        let Ok(path) = self.cover_cache_path(cover_art_id, Some(LARGE_COVER_ART_SIZE)) else {
            return;
        };
        if path.exists() {
            return;
        }

        let Ok(client_guard) = self.client.try_lock() else {
            return;
        };
        let Some(client) = client_guard.clone() else {
            return;
        };
        drop(client_guard);

        let cover_art_id = cover_art_id.to_string();
        tokio::spawn(async move {
            let Ok(bytes) = client
                .get_cover_art(&cover_art_id, Some(LARGE_COVER_ART_SIZE))
                .await
            else {
                return;
            };
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(path, bytes);
        });
    }

    fn cached_song_path(&self, song_id: &str) -> CoreResult<Option<PathBuf>> {
        let mp3_path = self.audio_cache_path(song_id, MOBILE_PLAYBACK_FORMAT)?;
        if mp3_path.exists() {
            return Ok(Some(mp3_path));
        }

        let conn = db::open_connection(&self.db_path)?;
        let saved_path = conn
            .query_row(
                "SELECT path FROM download_items
                 WHERE song_id = ?1 AND status = 'downloaded' AND path IS NOT NULL
                 ORDER BY updated_at DESC LIMIT 1",
                [song_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        if let Some(path) = saved_path {
            let path = PathBuf::from(path);
            if path.exists() && is_mobile_playback_cache_path(&path) {
                return Ok(Some(path));
            }
        }

        Ok(None)
    }

    fn audio_cache_entries(&self) -> CoreResult<Vec<(PathBuf, u64)>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(self.audio_cache_dir()?)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_mobile_playback_cache_path(&path) {
                entries.push((path, entry.metadata()?.len()));
            }
        }
        Ok(entries)
    }

    fn protected_audio_cache_paths(&self) -> CoreResult<HashSet<PathBuf>> {
        let conn = db::open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT ps.song_id
             FROM playlist_songs ps
             JOIN playlists p ON p.id = ps.playlist_id
             WHERE p.offline_saved_at IS NOT NULL",
        )?;
        let song_ids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let mut paths = HashSet::new();
        for song_id in song_ids {
            paths.insert(self.audio_cache_path(&song_id, MOBILE_PLAYBACK_FORMAT)?);
        }
        Ok(paths)
    }

    /// Cache file backing the track that is playing right now.
    ///
    /// Automatic eviction must skip it: on Windows the file cannot be unlinked while the
    /// decoder holds it open, and elsewhere evicting it silently drops the download record
    /// for a track the user is still listening to.
    fn active_playback_cache_path(&self) -> CoreResult<Option<PathBuf>> {
        let Some(song_id) = self.get_playback_state()?.current_song_id else {
            return Ok(None);
        };
        self.cached_song_path(&song_id)
    }

    fn song_protected_by_saved_playlist(&self, song_id: &str) -> CoreResult<bool> {
        let conn = db::open_connection(&self.db_path)?;
        conn.query_row(
            "SELECT EXISTS (
                SELECT 1
                FROM playlist_songs ps
                JOIN playlists p ON p.id = ps.playlist_id
                WHERE ps.song_id = ?1 AND p.offline_saved_at IS NOT NULL
             )",
            [song_id],
            |row| row.get::<_, bool>(0),
        )
        .map_err(Into::into)
    }

    fn max_cache_size(&self) -> CoreResult<u64> {
        let conn = db::open_connection(&self.db_path)?;
        let value = sync_value(&conn, "setting_max_cache_size")?
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5 * 1024 * 1024 * 1024);
        Ok(value.clamp(500 * 1024 * 1024, 50 * 1024 * 1024 * 1024))
    }

    fn set_setting(&self, key: &str, value: &str) -> CoreResult<()> {
        let conn = db::open_connection(&self.db_path)?;
        write_sync_value(&conn, &format!("setting_{key}"), value)
    }

    fn enforce_audio_cache_limit(&self) -> CoreResult<()> {
        self.enforce_audio_cache_limit_to(self.max_cache_size()?)
    }

    fn enforce_audio_cache_limit_to(&self, max_size: u64) -> CoreResult<()> {
        let _cache_guard = cache_mutation_guard()?;
        let mut entries = self.audio_cache_entries()?;
        let mut protected_paths = self.protected_audio_cache_paths()?;
        if let Some(active_path) = self.active_playback_cache_path()? {
            protected_paths.insert(active_path);
        }
        let mut total_size: u64 = entries.iter().map(|(_, size)| *size).sum();
        if total_size <= max_size {
            return Ok(());
        }

        entries.sort_by_key(|(path, _)| {
            path.metadata()
                .and_then(|metadata| metadata.accessed().or_else(|_| metadata.modified()))
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });

        for (path, size) in entries {
            if total_size <= max_size {
                break;
            }
            if protected_paths.contains(&path) {
                continue;
            }
            let path_string = path.to_string_lossy().to_string();
            let conn = db::open_connection(&self.db_path)?;
            let song_id = conn
                .query_row(
                    "SELECT song_id FROM download_items WHERE path = ?1 LIMIT 1",
                    [&path_string],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    total_size = total_size.saturating_sub(size);
                    if let Some(song_id) = song_id {
                        self.emit_cache_state_event(CacheStateEvent::CachedChanged {
                            song_id,
                            cached: false,
                        });
                    } else {
                        self.emit_cache_state_event(CacheStateEvent::Reconcile);
                    }
                    conn.execute("DELETE FROM download_items WHERE path = ?1", [&path_string])?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.emit_cache_state_event(CacheStateEvent::Reconcile);
                }
                Err(error) => {
                    // The file is still in use (or otherwise locked), so leave its size and
                    // download record intact and keep evicting the rest.
                    warn!(
                        "skipping cached audio file that could not be evicted: {} ({error})",
                        path.display()
                    );
                }
            }
        }

        Ok(())
    }

    fn record_download(&self, record: DownloadRecord<'_>) -> CoreResult<()> {
        let conn = db::open_connection(&self.db_path)?;
        let bytes = i64::try_from(record.bytes).map_err(|_| {
            CoreError::InvalidInput("download size exceeds SQLite range".to_string())
        })?;
        conn.execute(
            "INSERT OR REPLACE INTO download_items
             (entity_type, entity_id, song_id, status, path, bytes, error, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.entity_type,
                record.entity_id,
                record.song_id,
                record.status,
                record.path.map(|path| path.to_string_lossy().to_string()),
                bytes,
                record.error,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    fn playback_markers(&self) -> CoreResult<PlaybackMarkers> {
        let conn = db::open_connection(&self.db_path)?;
        let markers = conn
            .query_row(
                "SELECT app_volume, now_playing_song_id, scrobbled_song_id
                 FROM playback_state WHERE id = 1",
                [],
                |row| {
                    Ok(PlaybackMarkers {
                        app_volume: row.get(0)?,
                        now_playing_song_id: row.get(1)?,
                        scrobbled_song_id: row.get(2)?,
                    })
                },
            )
            .optional()?;

        Ok(markers.unwrap_or(PlaybackMarkers {
            app_volume: 1.0,
            now_playing_song_id: None,
            scrobbled_song_id: None,
        }))
    }

    fn save_playback_state(&self, state: &PlaybackStateWrite) -> CoreResult<PlaybackState> {
        let conn = db::open_connection(&self.db_path)?;
        let updated_at = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO playback_state
             (id, current_song_id, position_seconds, duration_seconds, was_playing, app_volume,
              now_playing_song_id, scrobbled_song_id, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                &state.song_id,
                state.position_seconds.max(0.0),
                state.duration_seconds.max(0.0),
                i64::from(state.was_playing),
                state.app_volume.clamp(0.0, 2.0),
                &state.now_playing_song_id,
                &state.scrobbled_song_id,
                updated_at
            ],
        )?;
        self.get_playback_state()
    }

    async fn retry_lastfm_queue_inner(&self, include_not_due: bool) -> CoreResult<usize> {
        let _retry_guard = self.lastfm_retry_lock.lock().await;
        lastfm::retry_queue(&self.db_path, include_not_due).await
    }

    fn with_queue_mutation_state(
        &self,
        mutate: impl FnOnce(&mut PlayQueue) -> CoreResult<()>,
    ) -> CoreResult<QueueState> {
        self.with_queue_mutation_result(|queue| {
            mutate(queue)?;
            Ok(())
        })
        .map(|((), state)| state)
    }

    fn with_queue_mutation_result<T>(
        &self,
        mutate: impl FnOnce(&mut PlayQueue) -> CoreResult<T>,
    ) -> CoreResult<(T, QueueState)> {
        let mut queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
        let result = mutate(&mut queue)?;
        let state = QueueState::from_queue(&mut queue);
        let save_result = db::save_queue(&self.db_path, &state, queue.original_order());
        self.queue_revision.fetch_add(1, Ordering::AcqRel);
        save_result?;
        Ok((result, state))
    }
}

fn handle_empty_incremental_scan(
    db_path: &Path,
    previous_head_album_id: Option<&str>,
    newest_scan: &NewestScanResult,
) -> CoreResult<()> {
    if let Some(head_album_id) = newest_scan.head_album_id.as_deref()
        && previous_head_album_id != Some(head_album_id)
    {
        let conn = db::open_connection(db_path)?;
        write_sync_value(&conn, NEWEST_HEAD_ALBUM_KEY, head_album_id)?;
    }

    match previous_head_album_id {
        Some(previous_head_album_id)
            if newest_scan.stop_reason == NewestScanStopReason::ReachedPreviousHead =>
        {
            info!(
                "Incremental sync skipped: no albums found before previous newest head ({previous_head_album_id})"
            );
        }
        Some(previous_head_album_id) => {
            info!(
                "Incremental sync skipped: newest-album feed exhausted before previous newest head ({previous_head_album_id})"
            );
        }
        None => {
            info!(
                "Incremental sync skipped: no previous newest head recorded and newest feed contained no unknown albums"
            );
        }
    }

    Ok(())
}

async fn fetch_incremental_library_data(
    client: Client,
    candidates: &[NewestAlbumCandidate],
    local_artists: &HashMap<String, LocalArtistRow>,
) -> CoreResult<LibrarySyncData> {
    let candidate_album_ids = candidates
        .iter()
        .map(|candidate| candidate.album_id.clone())
        .collect::<HashSet<_>>();
    let mut artist_names_by_id = HashMap::new();
    let mut artists_to_refresh = HashSet::new();

    for candidate in candidates {
        artists_to_refresh.insert(candidate.artist_id.clone());
        if let Some(name) = &candidate.artist_name {
            artist_names_by_id
                .entry(candidate.artist_id.clone())
                .or_insert(name.clone());
        }
    }

    let mut artist_ids = artists_to_refresh.into_iter().collect::<Vec<_>>();
    artist_ids.sort();
    let mut artist_fetches =
        fetch_artist_albums_bounded(client.clone(), artist_ids, ARTIST_FETCH_CONCURRENCY).await;
    artist_fetches.sort_by(|left, right| left.0.cmp(&right.0));

    let mut artists = Vec::new();
    let mut albums = Vec::new();
    let mut album_fetch_requests = Vec::new();

    for (artist_id, artist_result) in artist_fetches {
        let artist_albums = match artist_result {
            Ok(albums) => albums,
            Err(error) => {
                return Err(CoreError::Subsonic(format!(
                    "Incremental sync failed to fetch artist {artist_id}: {error}"
                )));
            }
        };
        let artist_name = artist_names_by_id
            .get(&artist_id)
            .cloned()
            .or_else(|| {
                local_artists
                    .get(&artist_id)
                    .map(|artist| artist.name.clone())
            })
            .unwrap_or_else(|| "Unknown Artist".to_string());
        let cover_art = local_artists
            .get(&artist_id)
            .and_then(|artist| artist.cover_art_id.clone());

        artists.push(SyncArtistData {
            id: artist_id.clone(),
            name: artist_name,
            album_count: i32::try_from(artist_albums.len()).map_err(|_| {
                CoreError::InvalidInput("artist contains too many albums".to_string())
            })?,
            cover_art,
        });

        for album in artist_albums {
            if candidate_album_ids.contains(&album.id) {
                album_fetch_requests.push(AlbumFetchRequest {
                    album_id: album.id.clone(),
                    artist_id: artist_id.clone(),
                    album_year: album.year,
                });
                albums.push(SyncAlbumData {
                    id: album.id,
                    artist_id: artist_id.clone(),
                    name: album.name,
                    year: album.year,
                    song_count: album.song_count,
                    duration: Some(album.duration),
                    cover_art: album.cover_art,
                });
            }
        }
    }

    let imported_album_ids = album_fetch_requests
        .iter()
        .map(|request| request.album_id.clone())
        .collect();
    ensure_incremental_albums_complete(&candidate_album_ids, &imported_album_ids, "artist fetches")
        .map_err(CoreError::Subsonic)?;

    let songs = fetch_incremental_songs(client, album_fetch_requests).await?;
    Ok(LibrarySyncData {
        artists,
        albums,
        songs,
    })
}

async fn fetch_incremental_songs(
    client: Client,
    album_fetch_requests: Vec<AlbumFetchRequest>,
) -> CoreResult<Vec<SyncSongData>> {
    let expected_album_ids = album_fetch_requests
        .iter()
        .map(|request| request.album_id.clone())
        .collect::<HashSet<_>>();
    let mut album_fetches =
        fetch_album_songs_bounded(client, album_fetch_requests, ALBUM_FETCH_CONCURRENCY).await;
    album_fetches.sort_by(|left, right| left.0.album_id.cmp(&right.0.album_id));
    let mut songs_data = Vec::new();
    let mut fetched_album_ids = HashSet::new();

    for (request, album_result) in album_fetches {
        let songs = album_result.map_err(|error| {
            CoreError::Subsonic(format!(
                "Incremental sync failed to fetch album {}: {error}",
                request.album_id
            ))
        })?;
        fetched_album_ids.insert(request.album_id.clone());
        songs_data.extend(songs.into_iter().map(|song| SyncSongData {
            id: song.id,
            album_id: request.album_id.clone(),
            artist_id: request.artist_id.clone(),
            title: song.title,
            track: song.track,
            disc_number: song.disc_number,
            duration: song.duration,
            bit_rate: song.bit_rate,
            size: song.size,
            suffix: song.suffix,
            content_type: song.content_type,
            path: song.path,
            year: song.year.or(request.album_year),
            genre: song.genre,
        }));
    }

    ensure_incremental_albums_complete(&expected_album_ids, &fetched_album_ids, "album fetches")
        .map_err(CoreError::Subsonic)?;

    Ok(songs_data)
}

async fn fetch_full_library_sync_data(client: &Client) -> CoreResult<LibrarySyncData> {
    let indexes = client
        .get_artists(None::<String>)
        .await
        .map_err(|e| CoreError::Subsonic(e.to_string()))?;
    let artists = indexes
        .into_iter()
        .flat_map(|index| index.artist)
        .collect::<Vec<_>>();
    info!("Full library sync fetched {} artists", artists.len());

    let mut sync_data = LibrarySyncData {
        artists: Vec::with_capacity(artists.len()),
        albums: Vec::new(),
        songs: Vec::new(),
    };

    for artist in artists {
        let artist_id = artist.id;
        let artist_detail = client
            .get_artist(&artist_id)
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;

        sync_data.artists.push(SyncArtistData {
            id: artist_id.clone(),
            name: artist.name,
            album_count: artist.album_count,
            cover_art: artist.cover_art,
        });

        for album in artist_detail.album {
            let album_id = album.id;
            let album_detail = client
                .get_album(&album_id)
                .await
                .map_err(|e| CoreError::Subsonic(e.to_string()))?;

            sync_data.albums.push(SyncAlbumData {
                id: album_id.clone(),
                artist_id: artist_id.clone(),
                name: album.name,
                year: album.year,
                song_count: album.song_count,
                duration: Some(album.duration),
                cover_art: album.cover_art,
            });

            for song in album_detail.song {
                sync_data.songs.push(SyncSongData {
                    id: song.id,
                    album_id: album_id.clone(),
                    artist_id: artist_id.clone(),
                    title: song.title,
                    track: song.track,
                    disc_number: song.disc_number.unwrap_or(1),
                    duration: song.duration,
                    bit_rate: song.bit_rate,
                    size: song.size,
                    suffix: song.suffix,
                    content_type: song.content_type,
                    path: song.path,
                    year: song.year,
                    genre: song.genre,
                });
            }
        }
    }

    Ok(sync_data)
}

fn apply_library_sync_data(
    tx: &rusqlite::Transaction<'_>,
    artists: &[SyncArtistData],
    albums: &[SyncAlbumData],
    songs: &[SyncSongData],
    synced_at: &str,
) -> CoreResult<()> {
    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO artists
             (id, name, album_count, cover_art_id, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for artist in artists {
            stmt.execute(params![
                artist.id,
                artist.name,
                artist.album_count,
                artist.cover_art,
                synced_at
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO albums
             (id, artist_id, name, year, song_count, duration, cover_art_id, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for album in albums {
            stmt.execute(params![
                album.id,
                album.artist_id,
                album.name,
                album.year,
                album.song_count,
                album.duration,
                album.cover_art,
                synced_at
            ])?;
        }
    }

    {
        let mut stmt = tx.prepare(
            "INSERT OR REPLACE INTO songs
             (id, album_id, artist_id, title, track_number, disc_number, duration,
              bit_rate, size, suffix, content_type, path, year, genre, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        )?;
        for song in songs {
            stmt.execute(params![
                song.id,
                song.album_id,
                song.artist_id,
                song.title,
                song.track,
                song.disc_number,
                song.duration,
                song.bit_rate,
                song.size,
                song.suffix,
                song.content_type,
                song.path,
                song.year,
                song.genre,
                synced_at
            ])?;
        }
    }

    Ok(())
}

async fn fetch_newest_head_album_id(client: &Client) -> CoreResult<Option<String>> {
    let newest = client
        .get_album_list2(Order::Newest, Some(1), Some(0), None::<String>)
        .await
        .map_err(|e| CoreError::Subsonic(e.to_string()))?;
    Ok(newest.first().map(|album| album.id.clone()))
}

async fn fetch_newest_album_candidates(
    client: &Client,
    previous_head_album_id: Option<&str>,
    known_album_ids: &HashSet<String>,
) -> CoreResult<NewestScanResult> {
    let mut head_album_id = None;
    let mut candidates = Vec::new();
    let mut offset = 0usize;
    let mut stop_reason = NewestScanStopReason::ExhaustedNewestFeed;

    loop {
        let page = client
            .get_album_list2(
                Order::Newest,
                Some(NEWEST_ALBUMS_PAGE_SIZE),
                Some(offset),
                None::<String>,
            )
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?
            .into_iter()
            .map(|album| NewestAlbumPageEntry {
                id: album.id,
                artist_id: album.artist_id,
                artist_name: album.artist,
            })
            .collect::<Vec<_>>();

        if page.is_empty() {
            break;
        }

        if head_album_id.is_none() {
            head_album_id = page.first().map(|album| album.id.clone());
        }

        let page_len = page.len();
        let page_scan = scan_newest_album_page(&page, previous_head_album_id, known_album_ids);
        candidates.extend(page_scan.candidates);

        if page_scan.reached_previous_head {
            stop_reason = NewestScanStopReason::ReachedPreviousHead;
            break;
        }

        if page_len < NEWEST_ALBUMS_PAGE_SIZE {
            break;
        }

        offset = offset.saturating_add(page_len);
    }

    Ok(NewestScanResult {
        head_album_id,
        candidates,
        stop_reason,
    })
}

fn scan_newest_album_page(
    page: &[NewestAlbumPageEntry],
    previous_head_album_id: Option<&str>,
    known_album_ids: &HashSet<String>,
) -> NewestPageScanResult {
    let mut candidates = Vec::new();

    for album in page {
        if previous_head_album_id == Some(album.id.as_str()) {
            return NewestPageScanResult {
                candidates,
                reached_previous_head: true,
            };
        }

        if known_album_ids.contains(&album.id) {
            continue;
        }

        let Some(artist_id) = album.artist_id.clone() else {
            warn!(
                "Skipping newest album {} because server did not provide artist_id",
                album.id
            );
            continue;
        };

        candidates.push(NewestAlbumCandidate {
            album_id: album.id.clone(),
            artist_id,
            artist_name: album.artist_name.clone(),
        });
    }

    NewestPageScanResult {
        candidates,
        reached_previous_head: false,
    }
}

fn ensure_incremental_albums_complete(
    candidate_album_ids: &HashSet<String>,
    completed_album_ids: &HashSet<String>,
    stage: &str,
) -> Result<(), String> {
    let mut missing_album_ids = candidate_album_ids
        .difference(completed_album_ids)
        .cloned()
        .collect::<Vec<_>>();
    missing_album_ids.sort();

    if missing_album_ids.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Incremental sync incomplete after {stage}; missing candidate albums: {}",
            missing_album_ids.join(", ")
        ))
    }
}

async fn fetch_artist_albums_bounded(
    client: Client,
    artist_ids: Vec<String>,
    concurrency: usize,
) -> Vec<(String, Result<Vec<RemoteAlbumSummary>, String>)> {
    let mut join_set = JoinSet::new();
    let mut pending_artist_ids = artist_ids.into_iter();
    let mut results = Vec::new();
    let concurrency = concurrency.max(1);

    for _ in 0..concurrency {
        let Some(artist_id) = pending_artist_ids.next() else {
            break;
        };
        spawn_artist_fetch(&mut join_set, client.clone(), artist_id);
    }

    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok(result) => results.push(result),
            Err(error) => warn!("Artist fetch task failed: {error}"),
        }

        if let Some(artist_id) = pending_artist_ids.next() {
            spawn_artist_fetch(&mut join_set, client.clone(), artist_id);
        }
    }

    results
}

fn spawn_artist_fetch(
    join_set: &mut JoinSet<(String, Result<Vec<RemoteAlbumSummary>, String>)>,
    client: Client,
    artist_id: String,
) {
    join_set.spawn(async move {
        let result = client
            .get_artist(&artist_id)
            .await
            .map(|artist_detail| {
                artist_detail
                    .album
                    .into_iter()
                    .map(|album| RemoteAlbumSummary {
                        id: album.id,
                        name: album.name,
                        year: album.year,
                        song_count: album.song_count,
                        duration: album.duration,
                        cover_art: album.cover_art,
                    })
                    .collect()
            })
            .map_err(|e| e.to_string());
        (artist_id, result)
    });
}

async fn fetch_album_songs_bounded(
    client: Client,
    requests: Vec<AlbumFetchRequest>,
    concurrency: usize,
) -> Vec<(AlbumFetchRequest, Result<Vec<RemoteSong>, String>)> {
    let mut join_set = JoinSet::new();
    let mut pending_requests = requests.into_iter();
    let mut results = Vec::new();
    let concurrency = concurrency.max(1);

    for _ in 0..concurrency {
        let Some(request) = pending_requests.next() else {
            break;
        };
        spawn_album_fetch(&mut join_set, client.clone(), request);
    }

    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok(result) => results.push(result),
            Err(error) => warn!("Album fetch task failed: {error}"),
        }

        if let Some(request) = pending_requests.next() {
            spawn_album_fetch(&mut join_set, client.clone(), request);
        }
    }

    results
}

fn spawn_album_fetch(
    join_set: &mut JoinSet<(AlbumFetchRequest, Result<Vec<RemoteSong>, String>)>,
    client: Client,
    request: AlbumFetchRequest,
) {
    join_set.spawn(async move {
        let result = client
            .get_album(&request.album_id)
            .await
            .map(|album_detail| {
                album_detail
                    .song
                    .into_iter()
                    .map(|song| RemoteSong {
                        id: song.id,
                        title: song.title,
                        track: song.track,
                        disc_number: song.disc_number.unwrap_or(1),
                        duration: song.duration,
                        bit_rate: song.bit_rate,
                        size: song.size,
                        suffix: song.suffix,
                        content_type: song.content_type,
                        path: song.path,
                        year: song.year,
                        genre: song.genre,
                    })
                    .collect()
            })
            .map_err(|e| e.to_string());
        (request, result)
    });
}

fn load_local_artists(conn: &Connection) -> CoreResult<HashMap<String, LocalArtistRow>> {
    let mut stmt = conn.prepare("SELECT id, name, cover_art_id FROM artists")?;
    let mut artists = HashMap::new();

    for row in stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            LocalArtistRow {
                name: row.get(1)?,
                cover_art_id: row.get(2)?,
            },
        ))
    })? {
        let (artist_id, artist) = row?;
        artists.insert(artist_id, artist);
    }

    Ok(artists)
}

fn load_local_album_ids(conn: &Connection) -> CoreResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM albums")?;
    let mut album_ids = HashSet::new();

    for row in stmt.query_map([], |row| row.get::<_, String>(0))? {
        album_ids.insert(row?);
    }

    Ok(album_ids)
}

fn build_client(url: &str, username: &str, password: &str) -> Client {
    let auth = AuthBuilder::new(username, API_VERSION)
        .client_name(CLIENT_NAME)
        .hashed(password);
    Client::new(url, auth)
}

fn read_server_config(path: &Path) -> CoreResult<Option<ServerConfig>> {
    match std::fs::read_to_string(path) {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn write_server_config(path: &Path, config: &ServerConfig) -> CoreResult<()> {
    let json = serde_json::to_string(config)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn save_server_row(db_path: &Path, config: &ServerConfig) -> CoreResult<()> {
    let conn = db::open_connection(db_path)?;
    conn.execute(
        "INSERT OR REPLACE INTO server_config (id, url, username, last_connected_at)
         VALUES (1, ?1, ?2, ?3)",
        params![config.url, config.username, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

fn rows_collect<T>(rows: impl Iterator<Item = Result<T, rusqlite::Error>>) -> CoreResult<Vec<T>> {
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn album_list_order(list_type: &str) -> CoreResult<Order> {
    match list_type {
        "newest" => Ok(Order::Newest),
        "recent" => Ok(Order::Recent),
        "frequent" => Ok(Order::Frequent),
        "random" => Ok(Order::Random),
        "highest" => Ok(Order::Highest),
        "alphabetical_by_name" => Ok(Order::AlphabeticalByName),
        "alphabetical_by_artist" => Ok(Order::AlphabeticalByArtist),
        "starred" => Ok(Order::Starred),
        other => Err(CoreError::InvalidAlbumListType(other.to_string())),
    }
}

fn playlist_from_subsonic(playlist: submarine::data::Playlist) -> Playlist {
    Playlist {
        id: playlist.id,
        name: playlist.name,
        song_count: playlist.song_count,
        duration: playlist.duration,
        owner: playlist.owner,
        cover_art_id: playlist.cover_art,
        created_at: playlist.created.to_rfc3339(),
        changed_at: playlist.changed.to_rfc3339(),
        saved_offline: false,
        offline_saved_at: None,
    }
}

fn playlist_from_row(row: &rusqlite::Row<'_>) -> Result<Playlist, rusqlite::Error> {
    let offline_saved_at = row.get::<_, Option<String>>(8)?;
    Ok(Playlist {
        id: row.get(0)?,
        name: row.get(1)?,
        song_count: row.get(2)?,
        duration: row.get(3)?,
        owner: row.get(4)?,
        cover_art_id: row.get(5)?,
        created_at: row.get(6)?,
        changed_at: row.get(7)?,
        saved_offline: offline_saved_at.is_some(),
        offline_saved_at,
    })
}

fn playlist_song_ids_to_add(
    song_ids: Vec<String>,
    existing_song_ids: &HashSet<String>,
) -> Vec<String> {
    let mut seen_song_ids = existing_song_ids.clone();

    song_ids
        .into_iter()
        .fold(Vec::new(), |mut output, song_id| {
            if !song_id.trim().is_empty() && seen_song_ids.insert(song_id.clone()) {
                output.push(song_id);
            }
            output
        })
}

fn sync_value(conn: &Connection, key: &str) -> CoreResult<Option<String>> {
    conn.query_row(
        "SELECT value FROM sync_state WHERE key = ?1",
        [key],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(Into::into)
}

fn write_sync_value(conn: &Connection, key: &str, value: &str) -> CoreResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_state (key, value, updated_at)
         VALUES (?1, ?2, ?3)",
        params![key, value, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

fn parse_rfc3339_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

fn is_job_due(
    enabled: bool,
    interval_minutes: u32,
    last_attempt_at: Option<&str>,
    now: DateTime<Utc>,
) -> bool {
    if !enabled {
        return false;
    }

    let Some(last_attempt_at) = last_attempt_at else {
        return true;
    };

    let Some(last_attempt) = parse_rfc3339_utc(last_attempt_at) else {
        return true;
    };

    now.signed_duration_since(last_attempt) >= ChronoDuration::minutes(i64::from(interval_minutes))
}

fn compute_next_run_at(
    enabled: bool,
    interval_minutes: u32,
    last_attempt_at: Option<&str>,
    now: DateTime<Utc>,
) -> Option<String> {
    if !enabled {
        return None;
    }

    let Some(last_attempt) = last_attempt_at.and_then(parse_rfc3339_utc) else {
        return Some(now.to_rfc3339());
    };

    last_attempt
        .checked_add_signed(ChronoDuration::minutes(i64::from(interval_minutes)))
        .map(|next_run| next_run.with_timezone(&Utc).to_rfc3339())
}

fn prune_stale_library_rows(conn: &Connection, synced_at: &str) -> CoreResult<()> {
    conn.execute(
        "DELETE FROM playlist_songs
         WHERE song_id IN (SELECT id FROM songs WHERE synced_at <> ?1)",
        [synced_at],
    )?;
    conn.execute(
        "DELETE FROM normalization_data
         WHERE song_id IN (SELECT id FROM songs WHERE synced_at <> ?1)",
        [synced_at],
    )?;
    conn.execute("DELETE FROM songs WHERE synced_at <> ?1", [synced_at])?;
    conn.execute("DELETE FROM albums WHERE synced_at <> ?1", [synced_at])?;
    conn.execute("DELETE FROM artists WHERE synced_at <> ?1", [synced_at])?;
    Ok(())
}

fn sanitize_file_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .collect()
}

fn atomic_write_temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!(
        "{}.part",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("download")
    ))
}

struct PendingAtomicFile {
    path: PathBuf,
    cleanup_on_drop: bool,
}

impl PendingAtomicFile {
    fn new(destination: &Path) -> Self {
        let path = atomic_write_temporary_path(destination);
        let _ = std::fs::remove_file(&path);
        Self {
            path,
            cleanup_on_drop: true,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn commit(&mut self, destination: &Path) -> CoreResult<()> {
        std::fs::rename(&self.path, destination)?;
        self.cleanup_on_drop = false;
        Ok(())
    }
}

impl Drop for PendingAtomicFile {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn cover_cache_filename(cover_art_id: &str, size: Option<i32>) -> String {
    let safe_id = sanitize_file_component(cover_art_id);
    match size {
        Some(size) => format!("{safe_id}_{size}.jpg"),
        None => format!("{safe_id}.jpg"),
    }
}

fn push_unique_cover_art_size(sizes: &mut Vec<Option<i32>>, size: Option<i32>) {
    if !sizes.contains(&size) {
        sizes.push(size);
    }
}

fn cover_art_filename_matches(file_name: &str, sanitized_id: &str) -> bool {
    let Some(stem) = file_name.strip_suffix(".jpg") else {
        return false;
    };

    if stem == sanitized_id {
        return true;
    }

    stem.strip_prefix(&format!("{sanitized_id}_"))
        .is_some_and(|size| !size.is_empty() && size.chars().all(|ch| ch.is_ascii_digit()))
}

fn distinct_nonempty_cover_art_ids(ids: Vec<Option<String>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut output = Vec::new();

    for id in ids {
        let Some(id) = id else {
            continue;
        };
        let id = id.trim();
        if id.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        output.push(id.to_string());
    }

    output
}

fn path_to_file_uri(path: &Path) -> String {
    url::Url::from_file_path(path).map_or_else(
        |()| format!("file://{}", path.to_string_lossy()),
        |url| url.to_string(),
    )
}

fn should_prefetch_large_cover_art(size: Option<i32>) -> bool {
    size.is_some_and(|size| size > 0 && size < LARGE_COVER_ART_SIZE)
}

fn is_mobile_playback_cache_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(MOBILE_PLAYBACK_FORMAT))
}

struct PlaybackMarkers {
    app_volume: f64,
    now_playing_song_id: Option<String>,
    scrobbled_song_id: Option<String>,
}

struct GaplessTrackInfo {
    album_id: String,
    disc_number: i32,
    track_number: i32,
}

fn gapless_track_info(conn: &Connection, song_id: &str) -> CoreResult<Option<GaplessTrackInfo>> {
    let info = conn
        .query_row(
            "SELECT album_id, disc_number, track_number FROM songs WHERE id = ?1",
            [song_id],
            |row| {
                Ok(GaplessTrackInfo {
                    album_id: row.get(0)?,
                    disc_number: row.get::<_, Option<i32>>(1)?.unwrap_or(1),
                    track_number: row.get::<_, Option<i32>>(2)?.unwrap_or(0),
                })
            },
        )
        .optional()?;
    Ok(info)
}

#[derive(Clone, Copy)]
struct DownloadRecord<'a> {
    entity_type: &'a str,
    entity_id: &'a str,
    song_id: &'a str,
    status: &'a str,
    path: Option<&'a Path>,
    bytes: u64,
    error: Option<&'a str>,
}

struct PlaybackStateWrite {
    song_id: Option<String>,
    position_seconds: f64,
    duration_seconds: f64,
    was_playing: bool,
    app_volume: f64,
    now_playing_song_id: Option<String>,
    scrobbled_song_id: Option<String>,
}

// Preset fields are enums, so deserialization already constrains them.
pub(crate) fn clamp_audio_processing_settings(settings: &mut AudioProcessingSettings) {
    settings.target_lufs = settings.target_lufs.clamp(-24.0, -8.0);
    settings.preamp_db = settings.preamp_db.clamp(-12.0, 12.0);
    settings.crossfade_duration_ms = settings.crossfade_duration_ms.clamp(500, 15_000);
    settings.prefetch_count = settings.prefetch_count.clamp(1, 10);
    settings.equalizer_bands_db.resize(12, 0.0);
    settings.equalizer_bands_db.truncate(12);
    for band in &mut settings.equalizer_bands_db {
        *band = band.clamp(-12.0, 12.0);
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
    use super::{
        AudioProcessingSettings, BinauralPreset, CacheStateEvent, CacheStateEventSender,
        ConnectivitySettings, CoreError, DownloadInProgressGuard, DownloadRecord,
        DownloadRecordFinalizer, DueSyncJob, DynamicsPreset, InvalidatableCache,
        LARGE_COVER_ART_SIZE, MOBILE_PLAYBACK_FORMAT, NewestAlbumCandidate, NewestAlbumPageEntry,
        NewestPageScanResult, NormalizationMode, PendingAtomicFile, PlaybackProgress, ServerConfig,
        Song, StereodromeCore, SyncSettings, atomic_write_temporary_path, build_client,
        compute_next_run_at, cover_art_filename_matches, cover_cache_filename,
        distinct_nonempty_cover_art_ids, ensure_incremental_albums_complete, is_job_due,
        path_to_file_uri, playlist_song_ids_to_add, prune_stale_library_rows,
        scan_newest_album_page, should_prefetch_large_cover_art, write_sync_value,
    };
    use crate::queue::{PlayQueue, QueueItem, RepeatMode};
    use chrono::{Duration as ChronoDuration, Utc};
    use rusqlite::Connection;
    use std::collections::HashSet;
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn invalidatable_cache_rejects_values_from_an_old_generation() {
        let mut cache = InvalidatableCache::default();
        let old_generation = cache.generation;

        cache.invalidate();
        cache.store_if_current(old_generation, "stale");

        assert_eq!(cache.value, None);

        let current_generation = cache.generation;
        cache.store_if_current(current_generation, "fresh");

        assert_eq!(cache.value, Some("fresh"));
    }

    #[test]
    fn prefetches_large_cover_art_for_small_requests() {
        assert!(should_prefetch_large_cover_art(Some(128)));
        assert!(should_prefetch_large_cover_art(Some(
            LARGE_COVER_ART_SIZE - 1
        )));
    }

    #[test]
    fn streamed_download_commit_atomically_replaces_partial_path() {
        let data_dir = unique_temp_dir("streamed-download-commit");
        std::fs::create_dir_all(&data_dir).expect("test directory");
        let destination = data_dir.join("song.mp3");
        let mut pending = PendingAtomicFile::new(&destination);
        std::fs::write(pending.path(), b"streamed audio").expect("partial file writes");

        pending.commit(&destination).expect("partial file commits");

        assert!(!pending.path().exists());
        assert_eq!(
            std::fs::read(&destination).expect("committed file reads"),
            b"streamed audio"
        );
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn streamed_download_partial_is_removed_on_early_return() {
        let data_dir = unique_temp_dir("streamed-download-cleanup");
        std::fs::create_dir_all(&data_dir).expect("test directory");
        let destination = data_dir.join("song.mp3");
        let temporary = {
            let pending = PendingAtomicFile::new(&destination);
            std::fs::write(pending.path(), b"partial audio").expect("partial file writes");
            pending.path().to_path_buf()
        };

        assert!(!temporary.exists());
        assert!(!destination.exists());
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn skips_prefetch_for_large_or_unsized_requests() {
        assert!(!should_prefetch_large_cover_art(Some(LARGE_COVER_ART_SIZE)));
        assert!(!should_prefetch_large_cover_art(Some(
            LARGE_COVER_ART_SIZE + 1
        )));
        assert!(!should_prefetch_large_cover_art(Some(0)));
        assert!(!should_prefetch_large_cover_art(None));
    }

    #[test]
    fn queue_prefetch_plan_counts_cached_and_duplicate_targets_within_bound() {
        let data_dir = unique_temp_dir("bounded-prefetch-plan");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        *core.queue.lock().expect("queue lock") = PlayQueue::load(
            vec![
                prefetch_queue_item("current"),
                prefetch_queue_item("cached"),
                prefetch_queue_item("cached"),
                prefetch_queue_item("second"),
                prefetch_queue_item("outside-window"),
            ],
            Some(0),
            false,
            RepeatMode::Off,
        );
        let cached_path = core
            .audio_cache_path("cached", MOBILE_PLAYBACK_FORMAT)
            .expect("cache path");
        std::fs::write(cached_path, b"cached").expect("write cached song");

        let plan = core.queue_prefetch_plan(2).expect("prefetch plan");

        assert_eq!(plan.current_index, Some(0));
        assert_eq!(plan.song_ids, vec!["cached", "second"]);
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn queue_mutation_invalidates_prefetch_generation() {
        let data_dir = unique_temp_dir("prefetch-generation");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        *core.queue.lock().expect("queue lock") = PlayQueue::load(
            vec![prefetch_queue_item("current"), prefetch_queue_item("next")],
            Some(0),
            false,
            RepeatMode::Off,
        );
        let before = core.queue_prefetch_plan(1).expect("initial plan");

        core.add_to_queue(prefetch_queue_item("added"))
            .expect("queue mutation succeeds");
        let after = core.queue_prefetch_plan(1).expect("updated plan");

        assert_ne!(before.queue_revision, after.queue_revision);
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn completed_prefetch_plan_is_revalidated_after_cache_eviction() {
        let data_dir = unique_temp_dir("prefetch-plan-cache-revalidation");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        *core.queue.lock().expect("queue lock") = PlayQueue::load(
            vec![prefetch_queue_item("current"), prefetch_queue_item("next")],
            Some(0),
            false,
            RepeatMode::Off,
        );
        let path = core
            .audio_cache_path("next", MOBILE_PLAYBACK_FORMAT)
            .expect("cache path");
        std::fs::write(&path, b"cached").expect("write cached song");
        let plan = core.queue_prefetch_plan(1).expect("prefetch plan");

        assert!(
            core.queue_prefetch_plan_is_satisfied(&plan)
                .expect("satisfied plan")
        );
        std::fs::remove_file(path).expect("evict cached song");
        assert!(
            !core
                .queue_prefetch_plan_is_satisfied(&plan)
                .expect("revalidated plan")
        );
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[tokio::test]
    async fn cancelled_prefetch_plan_stops_before_network_work() {
        let data_dir = unique_temp_dir("cancelled-prefetch-plan");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        *core.queue.lock().expect("queue lock") = PlayQueue::load(
            vec![prefetch_queue_item("current"), prefetch_queue_item("next")],
            Some(0),
            false,
            RepeatMode::Off,
        );
        let plan = core.queue_prefetch_plan(1).expect("prefetch plan");
        let cancellation = super::PrefetchCancellationToken::new();
        cancellation.cancel();

        let outcome = core
            .run_queue_prefetch_plan(&plan, &cancellation)
            .await
            .expect("cancelled prefetch exits cleanly");

        assert!(!outcome.completed);
        assert!(outcome.statuses.is_empty());
        assert!(core.get_downloading_song_ids().is_empty());
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn interrupted_download_record_is_finalized_synchronously() {
        let data_dir = unique_temp_dir("interrupted-download-finalizer");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        core.record_download(DownloadRecord {
            entity_type: "song",
            entity_id: "interrupted-song",
            song_id: "interrupted-song",
            status: "downloading",
            path: None,
            bytes: 0,
            error: None,
        })
        .expect("record active download");

        drop(DownloadRecordFinalizer::new(
            &core.db_path,
            "interrupted-song",
        ));

        let conn = Connection::open(&core.db_path).expect("open database");
        let status = conn
            .query_row(
                "SELECT status FROM download_items WHERE song_id = 'interrupted-song'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("read finalized status");
        assert_eq!(status, "cancelled");
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn failed_prefetch_song_enters_cooldown_until_success() {
        let data_dir = unique_temp_dir("prefetch-cooldown");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");

        core.record_prefetch_failure("failed-song")
            .expect("record failure");
        assert!(
            core.prefetch_failure_is_cooling_down("failed-song")
                .expect("read cooldown")
        );

        core.clear_prefetch_failure("failed-song")
            .expect("clear failure");
        assert!(
            !core
                .prefetch_failure_is_cooling_down("failed-song")
                .expect("read cleared cooldown")
        );
        std::fs::remove_dir_all(data_dir).ok();
    }

    fn prefetch_queue_item(song_id: &str) -> QueueItem {
        QueueItem {
            song_id: song_id.to_string(),
            title: song_id.to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration: 180,
        }
    }

    #[test]
    fn cold_restore_preserves_queue_selection_and_playback_position() {
        let data_dir = unique_temp_dir("cold-restore-characterization");
        {
            let core = StereodromeCore::new(&data_dir).expect("core initializes");
            core.add_songs_to_queue(vec![
                prefetch_queue_item("song-a"),
                prefetch_queue_item("song-b"),
            ])
            .expect("queue is populated");
            core.play_queue_item(1).expect("second song is selected");
            core.save_playback_position(PlaybackProgress {
                song_id: "song-b".to_string(),
                position_seconds: 42.5,
                duration_seconds: 180.0,
                is_playing: true,
            })
            .expect("playback position is saved");
        }

        let restored = StereodromeCore::new(&data_dir).expect("core restores");
        let queue = restored.get_queue().expect("queue restores");
        let playback = restored.get_playback_state().expect("playback restores");

        assert_eq!(queue.items.len(), 2);
        assert_eq!(queue.current_index, Some(1));
        assert_eq!(queue.items[1].song_id, "song-b");
        assert_eq!(playback.current_song_id.as_deref(), Some("song-b"));
        assert!((playback.position_seconds - 42.5).abs() < f64::EPSILON);
        assert!(playback.was_playing);
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn rapid_queue_navigation_remains_ordered_and_durable() {
        let data_dir = unique_temp_dir("rapid-navigation-characterization");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        core.add_songs_to_queue(vec![
            prefetch_queue_item("song-a"),
            prefetch_queue_item("song-b"),
            prefetch_queue_item("song-c"),
        ])
        .expect("queue is populated");
        core.play_queue_item(0).expect("first song is selected");

        for _ in 0..20 {
            assert_eq!(
                core.play_next(Some(true))
                    .expect("next succeeds")
                    .as_ref()
                    .map(|item| item.song_id.as_str()),
                Some("song-b")
            );
            assert_eq!(
                core.play_previous()
                    .expect("previous succeeds")
                    .as_ref()
                    .map(|item| item.song_id.as_str()),
                Some("song-a")
            );
        }

        drop(core);
        let restored = StereodromeCore::new(&data_dir).expect("core restores");
        assert_eq!(
            restored.get_queue().expect("queue restores").current_index,
            Some(0)
        );
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn clear_queue_and_audio_settings_are_persisted_across_restart() {
        let data_dir = unique_temp_dir("clear-settings-characterization");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        core.add_to_queue(prefetch_queue_item("song-a"))
            .expect("queue item is added");
        core.play_queue_item(0).expect("song is selected");
        core.clear_queue().expect("queue clears");

        let settings = core
            .set_audio_processing_settings(AudioProcessingSettings {
                normalization_enabled: true,
                normalization_mode: NormalizationMode::Album,
                target_lufs: -100.0,
                preamp_db: 100.0,
                prevent_clipping: true,
                dynamics_enabled: true,
                dynamics_preset: DynamicsPreset::Heavy,
                binaural_enabled: true,
                binaural_preset: BinauralPreset::Strong,
                equalizer_enabled: true,
                equalizer_bands_db: vec![24.0],
                gapless_enabled: true,
                crossfade_enabled: true,
                crossfade_duration_ms: 100,
                prefetch_count: 100,
            })
            .expect("audio settings persist");
        assert_eq!(settings.normalization_mode, NormalizationMode::Album);
        assert!((settings.target_lufs + 24.0).abs() < f64::EPSILON);
        assert!((settings.preamp_db - 12.0).abs() < f64::EPSILON);
        assert_eq!(settings.equalizer_bands_db.len(), 12);

        drop(core);
        let restored = StereodromeCore::new(&data_dir).expect("core restores");
        assert!(
            restored
                .get_queue()
                .expect("queue restores")
                .items
                .is_empty()
        );
        assert_eq!(
            restored
                .get_audio_processing_settings()
                .expect("settings restore")
                .prefetch_count,
            10
        );
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn cover_cache_filename_sanitizes_cover_art_ids() {
        assert_eq!(
            cover_cache_filename("album/cover\\id", Some(128)),
            "album_cover_id_128.jpg"
        );
        assert_eq!(
            cover_cache_filename("album/cover\\id", None),
            "album_cover_id.jpg"
        );
    }

    #[test]
    fn cover_art_filename_matches_only_same_cover_id() {
        assert!(cover_art_filename_matches("album_128.jpg", "album"));
        assert!(cover_art_filename_matches("album.jpg", "album"));
        assert!(!cover_art_filename_matches("album_large.jpg", "album"));
        assert!(!cover_art_filename_matches("album-extra_128.jpg", "album"));
        assert!(!cover_art_filename_matches("album_128.png", "album"));
    }

    #[test]
    fn distinct_nonempty_cover_art_ids_preserves_first_occurrence_order() {
        let cover_art_ids = distinct_nonempty_cover_art_ids(vec![
            Some(" playlist-cover ".to_string()),
            Some("album-cover".to_string()),
            None,
            Some(String::new()),
            Some("album-cover".to_string()),
            Some("artist-cover".to_string()),
            Some("playlist-cover".to_string()),
        ]);

        assert_eq!(
            cover_art_ids,
            ["playlist-cover", "album-cover", "artist-cover"]
        );
    }

    #[test]
    fn fallback_cover_cache_paths_prefers_requested_size_then_large_size() {
        let data_dir = unique_temp_dir("cover-art-fallback-paths");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");

        let paths = core
            .fallback_cover_cache_paths("cover-id", Some(128))
            .expect("fallback paths");

        assert_eq!(
            paths[0],
            core.cover_cache_path("cover-id", Some(128))
                .expect("requested path")
        );
        assert_eq!(
            paths[1],
            core.cover_cache_path("cover-id", Some(LARGE_COVER_ART_SIZE))
                .expect("large path")
        );
        assert_eq!(
            paths.last().unwrap(),
            &core
                .cover_cache_path("cover-id", None)
                .expect("unsized path")
        );

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[tokio::test]
    async fn cover_art_uri_uses_large_cached_cover_in_manual_offline_mode() {
        let data_dir = unique_temp_dir("manual-offline-cover-cache-fallback");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        core.set_connectivity_settings(ConnectivitySettings {
            manual_offline_enabled: true,
        })
        .expect("save connectivity settings");
        let large_path = core
            .cover_cache_path("cover-id", Some(LARGE_COVER_ART_SIZE))
            .expect("large cover path");
        std::fs::write(&large_path, b"cover").expect("write large cover");

        let uri = core
            .get_cover_art_uri("cover-id".to_string(), Some(128))
            .await
            .expect("cached cover uri works offline");

        assert_eq!(uri, path_to_file_uri(&large_path));
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[tokio::test]
    async fn failed_session_restore_discards_stale_client() {
        let data_dir = unique_temp_dir("failed-restore-clears-client");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let config = ServerConfig {
            url: "http://127.0.0.1:1".to_string(),
            username: "user".to_string(),
            password: "password".to_string(),
        };
        *core.server_config.lock().expect("server config lock") = Some(config.clone());
        *core.client.lock().await = Some(build_client(
            &config.url,
            &config.username,
            &config.password,
        ));

        let status = core
            .restore_session()
            .await
            .expect("restore returns status");

        assert!(!status.connected);
        assert!(core.client.lock().await.is_none());
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn playlist_song_ids_to_add_skips_duplicate_entries() {
        let song_ids = playlist_song_ids_to_add(
            vec![
                "song-a".to_string(),
                "song-b".to_string(),
                "song-a".to_string(),
            ],
            &HashSet::new(),
        );

        assert_eq!(song_ids, ["song-a", "song-b"]);
    }

    #[test]
    fn playlist_song_ids_to_add_skips_existing_playlist_entries() {
        let song_ids = playlist_song_ids_to_add(
            vec![
                "song-a".to_string(),
                "song-b".to_string(),
                "song-c".to_string(),
            ],
            &HashSet::from(["song-a".to_string(), "song-c".to_string()]),
        );

        assert_eq!(song_ids, ["song-b"]);
    }

    #[test]
    fn sync_settings_clamp_matches_mobile_scheduler_bounds() {
        let settings = SyncSettings {
            incremental_enabled: true,
            incremental_interval_minutes: 1,
            full_reconcile_enabled: true,
            full_reconcile_interval_hours: 999,
        }
        .clamped();

        assert_eq!(settings.incremental_interval_minutes, 5);
        assert_eq!(settings.full_reconcile_interval_hours, 168);
    }

    #[test]
    fn disabled_sync_job_is_never_due_and_has_no_next_run() {
        let now = Utc::now();

        assert!(!is_job_due(false, 15, None, now));
        assert_eq!(compute_next_run_at(false, 15, None, now), None);
    }

    #[test]
    fn invalid_or_missing_last_attempt_is_due_now() {
        let now = Utc::now();

        assert!(is_job_due(true, 15, None, now));
        assert!(is_job_due(true, 15, Some("not-a-date"), now));
        assert_eq!(
            compute_next_run_at(true, 15, Some("not-a-date"), now),
            Some(now.to_rfc3339())
        );
    }

    #[test]
    fn sync_job_due_uses_attempt_interval() {
        let now = Utc::now();
        let recent = (now - ChronoDuration::minutes(14)).to_rfc3339();
        let stale = (now - ChronoDuration::minutes(15)).to_rfc3339();

        assert!(!is_job_due(true, 15, Some(&recent), now));
        assert!(is_job_due(true, 15, Some(&stale), now));
        assert_eq!(
            compute_next_run_at(true, 15, Some(&recent), now),
            Some((now + ChronoDuration::minutes(1)).to_rfc3339())
        );
    }

    #[test]
    fn due_sync_job_prefers_full_reconcile() {
        let data_dir = unique_temp_dir("due-sync-priority");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");

        core.set_sync_settings(SyncSettings {
            incremental_enabled: true,
            incremental_interval_minutes: 15,
            full_reconcile_enabled: true,
            full_reconcile_interval_hours: 24,
        })
        .expect("save sync settings");

        assert_eq!(
            core.next_due_library_sync_job().expect("read due job"),
            Some(DueSyncJob::FullReconcile)
        );
    }

    #[test]
    fn newest_album_page_scan_stops_at_previous_head() {
        let known_album_ids = HashSet::new();
        let result = scan_newest_album_page(
            &[
                newest_album("album-new", Some("artist-1"), Some("Artist One")),
                newest_album("album-old", Some("artist-2"), Some("Artist Two")),
                newest_album("album-older", Some("artist-3"), Some("Artist Three")),
            ],
            Some("album-old"),
            &known_album_ids,
        );

        assert_eq!(
            result,
            NewestPageScanResult {
                candidates: vec![NewestAlbumCandidate {
                    album_id: "album-new".to_string(),
                    artist_id: "artist-1".to_string(),
                    artist_name: Some("Artist One".to_string()),
                }],
                reached_previous_head: true,
            }
        );
    }

    #[test]
    fn newest_album_page_scan_skips_known_and_artistless_albums() {
        let known_album_ids = HashSet::from(["album-known".to_string()]);
        let result = scan_newest_album_page(
            &[
                newest_album("album-known", Some("artist-1"), Some("Artist One")),
                newest_album("album-missing-artist", None, None),
                newest_album("album-new", Some("artist-2"), None),
            ],
            None,
            &known_album_ids,
        );

        assert_eq!(
            result.candidates,
            vec![NewestAlbumCandidate {
                album_id: "album-new".to_string(),
                artist_id: "artist-2".to_string(),
                artist_name: None,
            }]
        );
        assert!(!result.reached_previous_head);
    }

    #[test]
    fn incomplete_incremental_import_reports_missing_candidate_albums() {
        let error = ensure_incremental_albums_complete(
            &HashSet::from([
                "album-3".to_string(),
                "album-1".to_string(),
                "album-2".to_string(),
            ]),
            &HashSet::from(["album-2".to_string()]),
            "artist fetches",
        )
        .expect_err("missing candidates must prevent checkpoint advancement");

        assert_eq!(
            error,
            "Incremental sync incomplete after artist fetches; missing candidate albums: album-1, album-3"
        );
    }

    #[tokio::test]
    async fn get_playlist_songs_reads_local_cache_without_connection() {
        let data_dir = unique_temp_dir("local-playlist-songs");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let conn = Connection::open(&core.db_path).expect("open test db");

        conn.execute(
            "INSERT INTO artists (id, name, synced_at)
             VALUES ('artist-1', 'Artist One', 'now')",
            [],
        )
        .expect("insert artist");
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at)
             VALUES ('album-1', 'artist-1', 'Album One', 'now')",
            [],
        )
        .expect("insert album");
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, disc_number, synced_at)
             VALUES
                ('song-1', 'album-1', 'artist-1', 'First Song', 1, 'now'),
                ('song-2', 'album-1', 'artist-1', 'Second Song', 1, 'now')",
            [],
        )
        .expect("insert songs");
        conn.execute(
            "INSERT INTO playlists (id, name, song_count, duration, created_at, changed_at, synced_at)
             VALUES ('playlist-1', 'Offline Mix', 2, 0, 'now', 'now', 'now')",
            [],
        )
        .expect("insert playlist");
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_id, position)
             VALUES
                ('playlist-1', 'song-2', 0),
                ('playlist-1', 'song-1', 1)",
            [],
        )
        .expect("insert playlist songs");

        let songs = core
            .get_playlist_songs("playlist-1".to_string())
            .await
            .expect("read local playlist songs");

        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].id, "song-2");
        assert_eq!(songs[0].artist.as_deref(), Some("Artist One"));
        assert_eq!(songs[0].album.as_deref(), Some("Album One"));
        assert_eq!(songs[1].id, "song-1");

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn save_playlist_songs_links_only_existing_library_songs() {
        let data_dir = unique_temp_dir("playlist-non-library-entry");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let conn = Connection::open(&core.db_path).expect("open test db");

        conn.execute(
            "INSERT INTO artists (id, name, synced_at)
             VALUES ('artist-1', 'Artist One', 'now')",
            [],
        )
        .expect("insert artist");
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at)
             VALUES ('album-1', 'artist-1', 'Album One', 'now')",
            [],
        )
        .expect("insert album");
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, disc_number, synced_at)
             VALUES ('song-1', 'album-1', 'artist-1', 'Library Song', 1, 'now')",
            [],
        )
        .expect("insert song");
        conn.execute(
            "INSERT INTO playlists (id, name, song_count, duration, created_at, changed_at, synced_at)
             VALUES ('playlist-1', 'Mixed Entries', 2, 0, 'now', 'now', 'now')",
            [],
        )
        .expect("insert playlist");

        let songs = vec![
            Song {
                id: "song-1".to_string(),
                album_id: "album-1".to_string(),
                artist_id: "artist-1".to_string(),
                title: "Library Song".to_string(),
                track_number: None,
                disc_number: 1,
                duration: None,
                bit_rate: None,
                size: None,
                suffix: None,
                content_type: None,
                path: None,
                year: None,
                genre: None,
                synced_at: "now".to_string(),
                artist: Some("Artist One".to_string()),
                album: Some("Album One".to_string()),
            },
            Song {
                id: "external-entry".to_string(),
                album_id: String::new(),
                artist_id: String::new(),
                title: "External Entry".to_string(),
                track_number: None,
                disc_number: 1,
                duration: None,
                bit_rate: None,
                size: None,
                suffix: None,
                content_type: None,
                path: None,
                year: None,
                genre: None,
                synced_at: "now".to_string(),
                artist: None,
                album: None,
            },
        ];

        core.save_playlist_songs("playlist-1", &songs)
            .expect("save playlist membership");

        assert_eq!(count_rows(&conn, "playlist_songs"), 1);
        assert_eq!(
            count_rows(&conn, "playlist_songs WHERE song_id = 'song-1'"),
            1
        );
        assert_eq!(count_rows(&conn, "songs WHERE id = 'external-entry'"), 0);

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[tokio::test]
    async fn get_playlists_lists_only_saved_local_playlists_without_connection() {
        let data_dir = unique_temp_dir("local-playlists");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let conn = Connection::open(&core.db_path).expect("open test db");

        conn.execute(
            "INSERT INTO artists (id, name, synced_at)
             VALUES ('artist-1', 'Artist One', 'now')",
            [],
        )
        .expect("insert artist");
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at)
             VALUES ('album-1', 'artist-1', 'Album One', 'now')",
            [],
        )
        .expect("insert album");
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, disc_number, synced_at)
             VALUES
                ('song-downloaded', 'album-1', 'artist-1', 'Downloaded Song', 1, 'now'),
                ('song-streaming', 'album-1', 'artist-1', 'Streaming Song', 1, 'now')",
            [],
        )
        .expect("insert songs");
        conn.execute(
            "INSERT INTO playlists (id, name, song_count, duration, created_at, changed_at, offline_saved_at, synced_at)
             VALUES
                ('playlist-downloaded', 'Downloaded Mix', 1, 0, 'now', 'now', 'now', 'now'),
                ('playlist-streaming', 'Streaming Mix', 1, 0, 'now', 'now', NULL, 'now')",
            [],
        )
        .expect("insert playlists");
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_id, position)
             VALUES
                ('playlist-downloaded', 'song-downloaded', 0),
                ('playlist-streaming', 'song-streaming', 0)",
            [],
        )
        .expect("insert playlist songs");
        conn.execute(
            "INSERT INTO download_items
             (entity_type, entity_id, song_id, status, path, bytes, updated_at)
             VALUES ('song', 'song-downloaded', 'song-downloaded', 'downloaded', '/tmp/song.mp3', 1, 'now')",
            [],
        )
        .expect("insert download item");

        let playlists = core.get_playlists().await.expect("read local playlists");

        assert_eq!(playlists.len(), 1);
        assert_eq!(playlists[0].id, "playlist-downloaded");
        assert!(playlists[0].saved_offline);
        assert_eq!(playlists[0].offline_saved_at.as_deref(), Some("now"));

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[tokio::test]
    async fn saved_playlist_reconcile_uses_cached_library_without_a_connection() {
        let data_dir = unique_temp_dir("saved-playlist-reconcile-characterization");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let conn = Connection::open(&core.db_path).expect("open test db");

        conn.execute(
            "INSERT INTO artists (id, name, synced_at) VALUES ('artist', 'Artist', 'now')",
            [],
        )
        .expect("insert artist");
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at)
             VALUES ('album', 'artist', 'Album', 'now')",
            [],
        )
        .expect("insert album");
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, synced_at)
             VALUES ('cached-song', 'album', 'artist', 'Cached Song', 'now')",
            [],
        )
        .expect("insert song");
        conn.execute(
            "INSERT INTO playlists
             (id, name, song_count, duration, created_at, changed_at, offline_saved_at, synced_at)
             VALUES ('saved-playlist', 'Saved Playlist', 1, 0, 'now', 'now', 'now', 'now')",
            [],
        )
        .expect("insert playlist");
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_id, position)
             VALUES ('saved-playlist', 'cached-song', 0)",
            [],
        )
        .expect("insert playlist song");
        let cache_path = core
            .audio_cache_path("cached-song", MOBILE_PLAYBACK_FORMAT)
            .expect("cache path");
        std::fs::write(cache_path, b"cached audio").expect("write cached song");

        let results = core
            .reconcile_saved_playlists_offline()
            .await
            .expect("saved playlist reconciliation succeeds");

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].playlist_id, "saved-playlist");
        assert_eq!(results[0].downloaded_count, 1);
        assert!(results[0].saved_offline);
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn download_guard_publishes_start_and_finish_events() {
        let (sender, receiver) = mpsc::channel();
        let song_id = format!("event-song-{}", std::process::id());

        {
            let _guard = DownloadInProgressGuard::new(
                &song_id,
                vec![CacheStateEventSender::External(sender)],
            );
            assert_eq!(
                receiver
                    .recv_timeout(Duration::from_secs(1))
                    .expect("download start event"),
                CacheStateEvent::DownloadingChanged {
                    song_id: song_id.clone(),
                    downloading: true,
                }
            );
        }

        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("download finish event"),
            CacheStateEvent::DownloadingChanged {
                song_id,
                downloading: false,
            }
        );
    }

    #[tokio::test]
    async fn cache_mutations_publish_incremental_events() {
        let data_dir = unique_temp_dir("cache-state-events");
        let (sender, receiver) = mpsc::channel();
        let core = StereodromeCore::new_with_cache_events(&data_dir, sender)
            .expect("core initializes with cache events");
        let song_id = "cached-song";
        let cache_path = core
            .audio_cache_path(song_id, MOBILE_PLAYBACK_FORMAT)
            .expect("cache path");
        std::fs::write(&cache_path, b"cached audio").expect("write cache file");

        core.download_song(song_id.to_string())
            .await
            .expect("cache hit succeeds");
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("cached event"),
            CacheStateEvent::CachedChanged {
                song_id: song_id.to_string(),
                cached: true,
            }
        );

        core.remove_cached_song(song_id.to_string())
            .expect("cache removal succeeds");
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("removed event"),
            CacheStateEvent::CachedChanged {
                song_id: song_id.to_string(),
                cached: false,
            }
        );

        core.clear_audio_cache().expect("cache clear succeeds");
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("cache reconciliation event"),
            CacheStateEvent::Reconcile
        );

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[tokio::test]
    async fn download_song_returns_existing_cache_without_connection() {
        let data_dir = unique_temp_dir("download-song-cache-hit");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let song_id = "cached-song";
        let cache_path = core
            .audio_cache_path(song_id, MOBILE_PLAYBACK_FORMAT)
            .expect("cache path");
        std::fs::write(&cache_path, b"cached audio").expect("write cache file");

        let status = core
            .download_song(song_id.to_string())
            .await
            .expect("cache hit works offline");

        assert!(status.cached);
        assert_eq!(status.song_id, song_id);
        assert_eq!(status.bytes, 12);
        assert_eq!(
            status.path.as_deref(),
            Some(path_to_file_uri(&cache_path).as_str())
        );

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn manual_offline_setting_persists_in_local_core_settings() {
        let data_dir = unique_temp_dir("manual-offline-setting");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");

        assert!(
            !core
                .get_connectivity_settings()
                .expect("default connectivity settings")
                .manual_offline_enabled
        );

        core.set_connectivity_settings(ConnectivitySettings {
            manual_offline_enabled: true,
        })
        .expect("save connectivity settings");

        let restored = StereodromeCore::new(&data_dir).expect("core reinitializes");
        assert!(
            restored
                .get_connectivity_settings()
                .expect("restored connectivity settings")
                .manual_offline_enabled
        );

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn stream_uri_uses_cached_file_in_manual_offline_mode() {
        let data_dir = unique_temp_dir("manual-offline-stream-cache");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        core.set_connectivity_settings(ConnectivitySettings {
            manual_offline_enabled: true,
        })
        .expect("save connectivity settings");
        let song_id = "cached-song";
        let cache_path = core
            .audio_cache_path(song_id, MOBILE_PLAYBACK_FORMAT)
            .expect("cache path");
        std::fs::write(&cache_path, b"cached audio").expect("write cache file");

        let uri = core
            .get_stream_uri(song_id.to_string())
            .expect("cached stream uri works offline");

        assert_eq!(uri, path_to_file_uri(&cache_path));
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[tokio::test]
    async fn manual_offline_rejects_uncached_network_only_requests() {
        let data_dir = unique_temp_dir("manual-offline-network-guards");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        core.set_connectivity_settings(ConnectivitySettings {
            manual_offline_enabled: true,
        })
        .expect("save connectivity settings");

        assert!(matches!(
            core.get_stream_uri("uncached-song".to_string()),
            Err(CoreError::OfflineMode)
        ));
        assert!(matches!(
            core.download_song("uncached-song".to_string()).await,
            Err(CoreError::OfflineMode)
        ));
        assert!(matches!(
            core.get_cover_art_uri("cover-id".to_string(), Some(128))
                .await,
            Err(CoreError::OfflineMode)
        ));
        assert_eq!(
            core.run_due_library_sync()
                .await
                .expect("offline due sync is a no-op"),
            None
        );
        assert!(matches!(
            core.begin_lastfm_auth().await,
            Err(CoreError::OfflineMode)
        ));

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn clear_audio_cache_keeps_saved_playlist_songs() {
        let data_dir = unique_temp_dir("clear-keeps-saved-playlist");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let conn = Connection::open(&core.db_path).expect("open test db");

        conn.execute(
            "INSERT INTO artists (id, name, synced_at)
             VALUES ('artist-1', 'Artist One', 'now')",
            [],
        )
        .expect("insert artist");
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at)
             VALUES ('album-1', 'artist-1', 'Album One', 'now')",
            [],
        )
        .expect("insert album");
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, disc_number, synced_at)
             VALUES
                ('song-saved', 'album-1', 'artist-1', 'Saved Song', 1, 'now'),
                ('song-cache', 'album-1', 'artist-1', 'Cache Song', 1, 'now')",
            [],
        )
        .expect("insert songs");
        conn.execute(
            "INSERT INTO playlists (id, name, song_count, duration, created_at, changed_at, offline_saved_at, synced_at)
             VALUES ('playlist-saved', 'Saved Mix', 1, 0, 'now', 'now', 'now', 'now')",
            [],
        )
        .expect("insert playlist");
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_id, position)
             VALUES ('playlist-saved', 'song-saved', 0)",
            [],
        )
        .expect("insert playlist song");

        let saved_path = core
            .audio_cache_path("song-saved", MOBILE_PLAYBACK_FORMAT)
            .expect("saved cache path");
        let cache_path = core
            .audio_cache_path("song-cache", MOBILE_PLAYBACK_FORMAT)
            .expect("regular cache path");
        std::fs::write(&saved_path, b"saved").expect("write saved cache");
        std::fs::write(&cache_path, b"cache").expect("write regular cache");

        core.clear_audio_cache().expect("clear cache");

        assert!(saved_path.exists());
        assert!(!cache_path.exists());

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn partial_downloads_are_excluded_from_cache_maintenance() {
        let data_dir = unique_temp_dir("cache-maintenance-ignores-partials");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let committed_path = core
            .audio_cache_path("committed", MOBILE_PLAYBACK_FORMAT)
            .expect("committed cache path");
        let partial_path = atomic_write_temporary_path(&committed_path);
        std::fs::write(&committed_path, b"committed audio").expect("write committed cache file");
        std::fs::write(&partial_path, b"active partial download")
            .expect("write partial cache file");

        let stats = core.get_audio_cache_stats().expect("cache stats");
        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.total_size, 15);

        core.enforce_audio_cache_limit_to(0)
            .expect("enforce cache limit");
        assert!(!committed_path.exists(), "committed entry is evicted");
        assert!(partial_path.exists(), "partial download survives eviction");

        core.clear_audio_cache().expect("clear cache");
        assert!(
            partial_path.exists(),
            "partial download survives cache clear"
        );

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn clear_and_manual_removal_keep_the_currently_playing_track() {
        let data_dir = unique_temp_dir("clear-keeps-playing");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let playing_path = core
            .audio_cache_path("playing", MOBILE_PLAYBACK_FORMAT)
            .expect("cache path");
        std::fs::write(&playing_path, b"playing audio").expect("write cache file");
        core.save_playback_position(PlaybackProgress {
            song_id: "playing".to_string(),
            position_seconds: 12.0,
            duration_seconds: 180.0,
            is_playing: true,
        })
        .expect("persist playback position");

        core.clear_audio_cache().expect("cache clear succeeds");
        assert!(playing_path.exists(), "cache clear preserves active source");
        let removal = core.remove_cached_song("playing".to_string());
        assert!(matches!(removal, Err(CoreError::InvalidInput(_))));
        assert!(
            playing_path.exists(),
            "manual removal preserves active source"
        );

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn eviction_keeps_the_currently_playing_track() {
        // Written oldest-first so "playing" is the entry the least-recently-used
        // ordering reaches first.
        fn seed_cache(
            core: &StereodromeCore,
        ) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
            let mut paths = Vec::new();
            for song_id in ["playing", "older", "newer"] {
                let path = core
                    .audio_cache_path(song_id, MOBILE_PLAYBACK_FORMAT)
                    .expect("cache path");
                std::fs::write(&path, vec![0u8; 100]).expect("write cache file");
                std::thread::sleep(Duration::from_millis(20));
                paths.push(path);
            }
            let mut paths = paths.into_iter();
            (
                paths.next().expect("playing"),
                paths.next().expect("older"),
                paths.next().expect("newer"),
            )
        }

        // Control: with nothing playing, the oldest entry is the one evicted.
        let idle_dir = unique_temp_dir("eviction-idle");
        let idle = StereodromeCore::new(&idle_dir).expect("core initializes");
        let (idle_oldest, idle_older, idle_newer) = seed_cache(&idle);
        idle.enforce_audio_cache_limit_to(250)
            .expect("enforce cache limit");
        assert!(!idle_oldest.exists(), "oldest entry is evicted first");
        assert!(idle_older.exists());
        assert!(idle_newer.exists());
        std::fs::remove_dir_all(&idle_dir).ok();

        // With that same track playing, eviction skips it and takes the next candidate.
        let data_dir = unique_temp_dir("eviction-playing");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let (playing, older, newer) = seed_cache(&core);
        core.save_playback_position(PlaybackProgress {
            song_id: "playing".to_string(),
            position_seconds: 12.0,
            duration_seconds: 180.0,
            is_playing: true,
        })
        .expect("persist playback position");

        core.enforce_audio_cache_limit_to(250)
            .expect("enforce cache limit");

        assert!(playing.exists(), "playing track survives eviction");
        assert!(!older.exists(), "eviction moves on to the next candidate");
        assert!(newer.exists());

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn playlist_offline_cover_art_ids_includes_playlist_album_and_artist_art() {
        let data_dir = unique_temp_dir("playlist-offline-cover-art-ids");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let conn = Connection::open(&core.db_path).expect("open test db");

        conn.execute(
            "INSERT INTO artists (id, name, cover_art_id, synced_at)
             VALUES
                ('artist-1', 'Artist One', 'artist-cover', 'now'),
                ('artist-2', 'Artist Two', 'artist-cover-2', 'now')",
            [],
        )
        .expect("insert artists");
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, cover_art_id, synced_at)
             VALUES
                ('album-1', 'artist-1', 'Album One', 'album-cover', 'now'),
                ('album-2', 'artist-2', 'Album Two', 'album-cover', 'now')",
            [],
        )
        .expect("insert albums");
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, disc_number, synced_at)
             VALUES
                ('song-1', 'album-1', 'artist-1', 'Song One', 1, 'now'),
                ('song-2', 'album-2', 'artist-2', 'Song Two', 1, 'now')",
            [],
        )
        .expect("insert songs");
        conn.execute(
            "INSERT INTO playlists
             (id, name, song_count, duration, owner, cover_art_id, created_at, changed_at, synced_at)
             VALUES ('playlist-1', 'Playlist One', 2, 0, NULL, 'playlist-cover', 'now', 'now', 'now')",
            [],
        )
        .expect("insert playlist");
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_id, position)
             VALUES
                ('playlist-1', 'song-1', 0),
                ('playlist-1', 'song-2', 1)",
            [],
        )
        .expect("insert playlist songs");

        let cover_art_ids = core
            .playlist_offline_cover_art_ids("playlist-1")
            .expect("cover art ids");

        assert_eq!(
            cover_art_ids,
            [
                "playlist-cover",
                "album-cover",
                "artist-cover",
                "artist-cover-2"
            ]
        );

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn offline_song_ids_include_only_cached_library_songs() {
        let data_dir = unique_temp_dir("offline-song-ids");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let conn = Connection::open(&core.db_path).expect("open test db");
        conn.execute(
            "INSERT INTO artists (id, name, synced_at) VALUES ('artist', 'Artist', 'now')",
            [],
        )
        .expect("insert artist");
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at)
             VALUES ('album', 'artist', 'Album', 'now')",
            [],
        )
        .expect("insert album");
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, synced_at)
             VALUES
                ('cached-song', 'album', 'artist', 'Cached Song', 'now'),
                ('uncached-song', 'album', 'artist', 'Uncached Song', 'now')",
            [],
        )
        .expect("insert songs");
        let cache_path = core
            .audio_cache_path("cached-song", MOBILE_PLAYBACK_FORMAT)
            .expect("cache path");
        std::fs::write(&cache_path, b"cached audio").expect("write cache file");
        conn.execute(
            "INSERT INTO download_items
             (entity_type, entity_id, song_id, status, path, bytes, error, updated_at)
             VALUES ('song', 'cached-song', 'cached-song', 'downloaded', ?1, 12, NULL, 'old')",
            [cache_path.to_string_lossy().to_string()],
        )
        .expect("insert cached download record");

        let song_ids = core.get_offline_song_ids().expect("offline song ids load");

        assert_eq!(song_ids, vec!["cached-song"]);
        let updated_at = conn
            .query_row(
                "SELECT updated_at FROM download_items WHERE song_id = 'cached-song'",
                [],
                |row| row.get::<_, String>(0),
            )
            .expect("read cached download timestamp");
        assert_eq!(updated_at, "old", "offline lookup must remain read-only");
        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn detects_gapless_eligible_adjacent_album_tracks() {
        let data_dir = unique_temp_dir("gapless-eligible");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        seed_gapless_songs(&core);

        assert!(
            core.songs_are_gapless_eligible("disc1-track1", "disc1-track2")
                .expect("same disc eligibility")
        );
        assert!(
            core.songs_are_gapless_eligible("disc1-track2", "disc2-track1")
                .expect("next disc eligibility")
        );
        assert!(
            !core
                .songs_are_gapless_eligible("disc1-track1", "disc1-track3")
                .expect("non-adjacent ineligible")
        );
        assert!(
            !core
                .songs_are_gapless_eligible("disc1-track2", "other-album-track1")
                .expect("different album ineligible")
        );

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn library_sync_status_includes_full_sync_state() {
        let data_dir = unique_temp_dir("library-full-sync-status");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let conn = Connection::open(&core.db_path).expect("open test db");

        write_sync_value(
            &conn,
            "library_full_last_attempt_at",
            "2026-01-01T00:00:00Z",
        )
        .expect("write full attempt");
        write_sync_value(
            &conn,
            "library_full_last_success_at",
            "2026-01-01T00:01:00Z",
        )
        .expect("write full success");
        write_sync_value(
            &conn,
            "library_incremental_last_success_at",
            "2026-01-02T00:00:00Z",
        )
        .expect("write incremental success");

        let status = core.get_library_sync_status().expect("read sync status");

        assert_eq!(
            status.full.last_attempt_at.as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(
            status.full.last_success_at.as_deref(),
            Some("2026-01-01T00:01:00Z")
        );
        assert_eq!(
            status.incremental.last_success_at.as_deref(),
            Some("2026-01-02T00:00:00Z")
        );

        std::fs::remove_dir_all(data_dir).ok();
    }

    #[test]
    fn prune_stale_library_rows_removes_missing_songs_and_dependents() {
        let data_dir = unique_temp_dir("prune-stale-library");
        let core = StereodromeCore::new(&data_dir).expect("core initializes");
        let conn = Connection::open(&core.db_path).expect("open test db");

        conn.execute(
            "INSERT INTO artists (id, name, synced_at)
             VALUES ('artist-stale', 'Stale Artist', 'old'), ('artist-keep', 'Keep Artist', 'now')",
            [],
        )
        .expect("insert artists");
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at)
             VALUES
                ('album-stale', 'artist-stale', 'Stale Album', 'old'),
                ('album-keep', 'artist-keep', 'Keep Album', 'now')",
            [],
        )
        .expect("insert albums");
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, synced_at)
             VALUES
                ('song-stale', 'album-stale', 'artist-stale', 'Stale Song', 'old'),
                ('song-keep', 'album-keep', 'artist-keep', 'Keep Song', 'now')",
            [],
        )
        .expect("insert songs");
        conn.execute(
            "INSERT INTO playlists (id, name, created_at, changed_at, synced_at)
             VALUES ('playlist-1', 'Playlist', 'now', 'now', 'now')",
            [],
        )
        .expect("insert playlist");
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_id, position)
             VALUES ('playlist-1', 'song-stale', 0)",
            [],
        )
        .expect("insert playlist song");
        conn.execute(
            "INSERT INTO normalization_data
             (song_id, track_loudness_lufs, track_peak, album_id, analyzed_at)
             VALUES ('song-stale', -14.0, 0.9, 'album-stale', 'now')",
            [],
        )
        .expect("insert normalization");

        prune_stale_library_rows(&conn, "now").expect("prune stale rows");

        assert_eq!(count_rows(&conn, "playlist_songs"), 0);
        assert_eq!(count_rows(&conn, "normalization_data"), 0);
        assert_eq!(count_rows(&conn, "songs WHERE id = 'song-stale'"), 0);
        assert_eq!(count_rows(&conn, "songs WHERE id = 'song-keep'"), 1);
        assert_eq!(count_rows(&conn, "albums WHERE id = 'album-stale'"), 0);
        assert_eq!(count_rows(&conn, "artists WHERE id = 'artist-stale'"), 0);

        std::fs::remove_dir_all(data_dir).ok();
    }

    fn unique_temp_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("stereodrome-{name}-{}-{nanos}", std::process::id()))
    }

    fn newest_album(
        id: &str,
        artist_id: Option<&str>,
        artist_name: Option<&str>,
    ) -> NewestAlbumPageEntry {
        NewestAlbumPageEntry {
            id: id.to_string(),
            artist_id: artist_id.map(str::to_string),
            artist_name: artist_name.map(str::to_string),
        }
    }

    fn seed_gapless_songs(core: &StereodromeCore) {
        let conn = Connection::open(&core.db_path).expect("open test db");
        conn.execute(
            "INSERT INTO artists (id, name, synced_at) VALUES ('artist', 'Artist', 'now')",
            [],
        )
        .expect("insert artist");
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at)
             VALUES ('album', 'artist', 'Album', 'now'), ('other', 'artist', 'Other', 'now')",
            [],
        )
        .expect("insert albums");
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, track_number, disc_number, synced_at)
             VALUES
                ('disc1-track1', 'album', 'artist', 'One', 1, 1, 'now'),
                ('disc1-track2', 'album', 'artist', 'Two', 2, 1, 'now'),
                ('disc1-track3', 'album', 'artist', 'Three', 3, 1, 'now'),
                ('disc2-track1', 'album', 'artist', 'Three', 1, 2, 'now'),
                ('other-album-track1', 'other', 'artist', 'Other One', 1, 1, 'now')",
            [],
        )
        .expect("insert songs");
    }

    fn count_rows(conn: &Connection, table_or_filter: &str) -> i64 {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM {table_or_filter}"),
            [],
            |row| row.get(0),
        )
        .expect("count rows")
    }
}
