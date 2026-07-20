use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use log::{debug, info, warn};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::task::JoinSet;

use crate::error::{AppError, AppResult, MutexExt};
use crate::search::{AlbumIndexData, ArtistIndexData, IndexManager, SongIndexData};
use crate::state::AppState;

const NEWEST_HEAD_ALBUM_KEY: &str = "library_newest_head_album_id";
const NEWEST_ALBUMS_PAGE_SIZE: usize = 200;
const INCREMENTAL_LAST_ATTEMPT_AT_KEY: &str = "library_incremental_last_attempt_at";
const INCREMENTAL_LAST_SUCCESS_AT_KEY: &str = "library_incremental_last_success_at";
const INCREMENTAL_LAST_ERROR_KEY: &str = "library_incremental_last_error";
const FULL_LAST_ATTEMPT_AT_KEY: &str = "library_full_last_attempt_at";
const FULL_LAST_SUCCESS_AT_KEY: &str = "library_full_last_success_at";
const FULL_LAST_ERROR_KEY: &str = "library_full_last_error";
const SYNC_SCHEDULER_POLL_INTERVAL: Duration = Duration::from_secs(30);
const SYNC_ALREADY_RUNNING_MESSAGE: &str = "Library sync already in progress";
const ARTIST_FETCH_CONCURRENCY: usize = 8;
const ALBUM_FETCH_CONCURRENCY: usize = 12;

fn usize_to_i32_saturating(value: usize) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanStatus {
    pub scanning: bool,
    pub count: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub album_count: i32,
    pub cover_art_id: Option<String>,
    pub synced_at: String,
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
    #[serde(rename = "artistName")]
    pub artist_name: Option<String>,
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
    // Joined fields
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub artists: usize,
    pub albums: usize,
    pub songs: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryContentUpdatedEvent {
    pub job: SyncJobKind,
    pub new_artists: usize,
    pub new_albums: usize,
    pub new_songs: usize,
    pub has_new_items: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncJobKind {
    Incremental,
    FullReconcile,
}

impl SyncJobKind {
    fn as_key(self) -> &'static str {
        match self {
            Self::Incremental => "incremental",
            Self::FullReconcile => "full_reconcile",
        }
    }

    fn last_attempt_key(self) -> &'static str {
        match self {
            Self::Incremental => INCREMENTAL_LAST_ATTEMPT_AT_KEY,
            Self::FullReconcile => FULL_LAST_ATTEMPT_AT_KEY,
        }
    }

    fn last_success_key(self) -> &'static str {
        match self {
            Self::Incremental => INCREMENTAL_LAST_SUCCESS_AT_KEY,
            Self::FullReconcile => FULL_LAST_SUCCESS_AT_KEY,
        }
    }

    fn last_error_key(self) -> &'static str {
        match self {
            Self::Incremental => INCREMENTAL_LAST_ERROR_KEY,
            Self::FullReconcile => FULL_LAST_ERROR_KEY,
        }
    }
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
    pub active_job: Option<SyncJobKind>,
    pub incremental: SyncJobStatus,
    pub full_reconcile: SyncJobStatus,
}

// Internal structs for collecting data before writing to DB
struct ArtistData {
    id: String,
    name: String,
    album_count: i32,
    cover_art: Option<String>,
}

struct AlbumData {
    id: String,
    artist_id: String,
    name: String,
    year: Option<i32>,
    song_count: i32,
    duration: Option<i32>,
    cover_art: Option<String>,
}

struct SongData {
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

fn sync_job_lock() -> &'static AtomicBool {
    static LOCK: OnceLock<AtomicBool> = OnceLock::new();
    LOCK.get_or_init(|| AtomicBool::new(false))
}

fn active_sync_job_state() -> &'static Mutex<Option<SyncJobKind>> {
    static ACTIVE_JOB: OnceLock<Mutex<Option<SyncJobKind>>> = OnceLock::new();
    ACTIVE_JOB.get_or_init(|| Mutex::new(None))
}

struct SyncJobGuard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncTrigger {
    Manual,
    Scheduled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct NewItemCounts {
    artists: usize,
    albums: usize,
    songs: usize,
}

impl NewItemCounts {
    fn has_new_items(self) -> bool {
        self.artists > 0 || self.albums > 0 || self.songs > 0
    }

    fn into_event(self, job: SyncJobKind) -> LibraryContentUpdatedEvent {
        LibraryContentUpdatedEvent {
            job,
            new_artists: self.artists,
            new_albums: self.albums,
            new_songs: self.songs,
            has_new_items: self.has_new_items(),
        }
    }
}

impl SyncJobGuard {
    fn acquire(kind: SyncJobKind) -> Option<Self> {
        if sync_job_lock()
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let mut active = active_sync_job_state().lock_recover();
            *active = Some(kind);
            Some(Self)
        } else {
            None
        }
    }
}

impl Drop for SyncJobGuard {
    fn drop(&mut self) {
        {
            let mut active = active_sync_job_state().lock_recover();
            *active = None;
        }
        sync_job_lock().store(false, Ordering::SeqCst);
    }
}

pub fn start_library_sync_scheduler(app_handle: AppHandle) {
    thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime for library sync scheduler");

        loop {
            thread::sleep(SYNC_SCHEDULER_POLL_INTERVAL);

            let state = app_handle.state::<AppState>();
            if crate::commands::settings::manual_offline_enabled(&app_handle)
                || !state.client.is_connected()
                || sync_job_lock().load(Ordering::SeqCst)
            {
                continue;
            }

            let settings = crate::commands::settings::read_sync_settings(&app_handle);
            let due_job = match next_due_sync_job(state.inner(), &settings) {
                Ok(job) => job,
                Err(e) => {
                    warn!("Failed to evaluate scheduled sync due state: {e}");
                    continue;
                }
            };

            let Some(job) = due_job else {
                continue;
            };

            if let Err(e) = runtime.block_on(run_sync_job_with_status(
                state.inner(),
                &app_handle,
                job,
                SyncTrigger::Scheduled,
            )) {
                warn!("Scheduled {} sync failed: {}", job.as_key(), e);
            }
        }
    });
}

#[tauri::command]
pub async fn sync_library(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<SyncResult> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(AppError::OfflineMode);
    }

    run_sync_job_with_status(
        state.inner(),
        &app_handle,
        SyncJobKind::Incremental,
        SyncTrigger::Manual,
    )
    .await
}

#[tauri::command]
pub async fn reconcile_library_state(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<SyncResult> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(AppError::OfflineMode);
    }

    run_sync_job_with_status(
        state.inner(),
        &app_handle,
        SyncJobKind::FullReconcile,
        SyncTrigger::Manual,
    )
    .await
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_library_sync_status(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<LibrarySyncStatus> {
    let settings = crate::commands::settings::read_sync_settings(&app_handle);
    read_library_sync_status(state.inner(), &settings)
}

async fn run_sync_job_with_status(
    state: &AppState,
    app_handle: &AppHandle,
    job: SyncJobKind,
    trigger: SyncTrigger,
) -> AppResult<SyncResult> {
    let guard = SyncJobGuard::acquire(job)
        .ok_or_else(|| AppError::Subsonic(SYNC_ALREADY_RUNNING_MESSAGE.to_string()))?;

    let started_at = Utc::now().to_rfc3339();
    {
        let db = state.db.lock_recover();
        set_sync_state(&db, job.last_attempt_key(), &started_at, &started_at)?;
        clear_sync_state(&db, job.last_error_key())?;
    }
    emit_library_sync_status_changed(state, app_handle);

    let result = match job {
        SyncJobKind::Incremental => run_incremental_sync(state).await,
        SyncJobKind::FullReconcile => run_full_reconcile_sync(state).await,
    };

    let finished_at = Utc::now().to_rfc3339();
    {
        let db = state.db.lock_recover();
        match &result {
            Ok(_) => {
                set_sync_state(&db, job.last_success_key(), &finished_at, &finished_at)?;
                clear_sync_state(&db, job.last_error_key())?;
            }
            Err(err) => {
                let error_message = err.to_string();
                set_sync_state(&db, job.last_error_key(), &error_message, &finished_at)?;
            }
        }
    }

    drop(guard);
    emit_library_sync_status_changed(state, app_handle);

    let (result, new_item_counts) = result?;

    if trigger == SyncTrigger::Scheduled {
        emit_library_content_updated(app_handle, new_item_counts.into_event(job));
    }

    Ok(result)
}

#[allow(clippy::too_many_lines)]
async fn run_incremental_sync(state: &AppState) -> AppResult<(SyncResult, NewItemCounts)> {
    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    let (previous_head_album_id, local_artists, local_album_ids) = {
        let db = state.db.lock_recover();
        (
            get_sync_state(&db, NEWEST_HEAD_ALBUM_KEY)?,
            load_local_artists(&db)?,
            load_local_album_ids(&db)?,
        )
    };
    let local_artist_ids: HashSet<String> = local_artists.keys().cloned().collect();

    let newest_scan =
        fetch_newest_album_candidates(state, previous_head_album_id.as_deref(), &local_album_ids)
            .await?;

    if newest_scan.candidates.is_empty() {
        if let Some(head_album_id) = newest_scan.head_album_id.as_deref()
            && previous_head_album_id.as_deref() != Some(head_album_id)
        {
            let now = Utc::now().to_rfc3339();
            let db = state.db.lock_recover();
            set_sync_state(&db, NEWEST_HEAD_ALBUM_KEY, head_album_id, &now)?;
        }

        match previous_head_album_id.as_deref() {
            Some(previous_head_album_id)
                if newest_scan.stop_reason == NewestScanStopReason::ReachedPreviousHead =>
            {
                info!(
                    "Library sync skipped: no albums found before previous newest head ({previous_head_album_id})"
                );
            }
            Some(previous_head_album_id) => {
                info!(
                    "Library sync skipped: newest-album feed exhausted before previous newest head ({previous_head_album_id})"
                );
            }
            None => {
                info!(
                    "Library sync skipped: no previous newest head recorded; newest-album feed contained no unknown albums"
                );
            }
        }

        return Ok((
            SyncResult {
                artists: 0,
                albums: 0,
                songs: 0,
            },
            NewItemCounts::default(),
        ));
    }

    let mut artists_data: Vec<ArtistData> = Vec::new();
    let mut albums_data: Vec<AlbumData> = Vec::new();
    let mut songs_data: Vec<SongData> = Vec::new();
    let candidate_album_ids: HashSet<String> = newest_scan
        .candidates
        .iter()
        .map(|c| c.album_id.clone())
        .collect();
    let mut artist_names_by_id: HashMap<String, String> = HashMap::new();
    let mut artists_to_refresh: HashSet<String> = HashSet::new();

    for candidate in &newest_scan.candidates {
        artists_to_refresh.insert(candidate.artist_id.clone());
        if let Some(name) = &candidate.artist_name {
            artist_names_by_id
                .entry(candidate.artist_id.clone())
                .or_insert(name.clone());
        }
    }

    let mut artists_to_refresh: Vec<String> = artists_to_refresh.into_iter().collect();
    artists_to_refresh.sort();
    let mut artist_fetches = fetch_artist_albums_bounded(
        state.client.clone(),
        artists_to_refresh,
        ARTIST_FETCH_CONCURRENCY,
    )
    .await;
    artist_fetches.sort_by(|left, right| left.0.cmp(&right.0));
    let mut album_fetch_requests = Vec::new();

    for (artist_id, artist_result) in artist_fetches {
        let artist_albums = match artist_result {
            Ok(albums) => albums,
            Err(e) => {
                warn!("Error fetching artist {artist_id}: {e}");
                continue;
            }
        };

        let artist_name = artist_names_by_id
            .get(&artist_id)
            .cloned()
            .or_else(|| local_artists.get(&artist_id).map(|a| a.name.clone()))
            .unwrap_or_else(|| "Unknown Artist".to_string());
        let cover_art = local_artists
            .get(&artist_id)
            .and_then(|a| a.cover_art_id.clone());

        artists_data.push(ArtistData {
            id: artist_id.clone(),
            name: artist_name,
            album_count: usize_to_i32_saturating(artist_albums.len()),
            cover_art,
        });

        for album in artist_albums {
            let album_id = album.id.clone();
            if !candidate_album_ids.contains(&album_id) {
                continue;
            }

            albums_data.push(AlbumData {
                id: album_id.clone(),
                artist_id: artist_id.clone(),
                name: album.name,
                year: album.year,
                song_count: album.song_count,
                duration: Some(album.duration),
                cover_art: album.cover_art,
            });
            album_fetch_requests.push(AlbumFetchRequest {
                album_id,
                artist_id: artist_id.clone(),
                album_year: album.year,
            });
        }
    }

    let mut album_fetches = fetch_album_songs_bounded(
        state.client.clone(),
        album_fetch_requests,
        ALBUM_FETCH_CONCURRENCY,
    )
    .await;
    album_fetches.sort_by(|left, right| left.0.album_id.cmp(&right.0.album_id));

    for (request, album_result) in album_fetches {
        match album_result {
            Ok(songs) => {
                for song in songs {
                    songs_data.push(SongData {
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
                    });
                }
            }
            Err(e) => warn!("Error fetching album {}: {}", request.album_id, e),
        }
    }

    info!(
        "Applying newest-album incremental sync: importing {} newest albums via {} artists (upserts: {} artists, {} albums, {} songs)",
        candidate_album_ids.len(),
        artists_data.len(),
        artists_data.len(),
        albums_data.len(),
        songs_data.len(),
    );

    let now = Utc::now().to_rfc3339();
    let db = state.db.lock_recover();

    db.execute("BEGIN IMMEDIATE", [])?;

    let result = (|| {
        for artist in &artists_data {
            db.execute(
                "INSERT OR REPLACE INTO artists (id, name, album_count, cover_art_id, synced_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![artist.id, artist.name, artist.album_count, artist.cover_art, &now],
            )?;
        }

        for album in &albums_data {
            db.execute(
                "INSERT OR REPLACE INTO albums (id, artist_id, name, year, song_count, duration, cover_art_id, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![album.id, album.artist_id, album.name, album.year, album.song_count, album.duration, album.cover_art, &now],
            )?;
        }

        for song in &songs_data {
            db.execute(
                "INSERT OR REPLACE INTO songs (id, album_id, artist_id, title, track_number, disc_number, duration, bit_rate, size, suffix, content_type, path, year, genre, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![song.id, song.album_id, song.artist_id, song.title, song.track, song.disc_number, song.duration, song.bit_rate, song.size, song.suffix, song.content_type, song.path, song.year, song.genre, &now],
            )?;
        }

        if let Some(head_album_id) = newest_scan.head_album_id.as_deref() {
            set_sync_state(&db, NEWEST_HEAD_ALBUM_KEY, head_album_id, &now)?;
        }

        Ok::<(), crate::error::AppError>(())
    })();

    match result {
        Ok(()) => db.execute("COMMIT", [])?,
        Err(e) => {
            let _ = db.execute("ROLLBACK", []);
            return Err(e);
        }
    };

    // Drop db lock before rebuilding search index.
    drop(db);

    update_search_index_for_incremental_sync(state, &artists_data, &albums_data, &songs_data)?;

    let refreshed_artist_ids: HashSet<String> = artists_data
        .iter()
        .map(|artist| artist.id.clone())
        .collect();
    let new_item_counts = count_incremental_new_items(
        &local_artist_ids,
        &refreshed_artist_ids,
        albums_data.len(),
        songs_data.len(),
    );

    Ok((
        SyncResult {
            artists: artists_data.len(),
            albums: albums_data.len(),
            songs: songs_data.len(),
        },
        new_item_counts,
    ))
}

#[allow(clippy::too_many_lines)]
async fn run_full_reconcile_sync(state: &AppState) -> AppResult<(SyncResult, NewItemCounts)> {
    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    let (local_artist_ids, local_album_ids, local_song_ids) = {
        let db = state.db.lock_recover();
        (
            load_local_artist_ids(&db)?,
            load_local_album_ids(&db)?,
            load_local_song_ids(&db)?,
        )
    };

    let artist_summaries = state
        .client
        .get_artists()
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    let mut artists_data: Vec<ArtistData> = Vec::new();
    let mut albums_data: Vec<AlbumData> = Vec::new();
    let mut songs_data: Vec<SongData> = Vec::new();
    let mut had_fetch_errors = false;
    let mut artist_summaries_by_id: HashMap<String, _> = artist_summaries
        .into_iter()
        .map(|summary| (summary.id.clone(), summary))
        .collect();
    let mut artist_ids: Vec<String> = artist_summaries_by_id.keys().cloned().collect();
    artist_ids.sort();
    let mut artist_fetches =
        fetch_artist_albums_bounded(state.client.clone(), artist_ids, ARTIST_FETCH_CONCURRENCY)
            .await;
    artist_fetches.sort_by(|left, right| left.0.cmp(&right.0));
    let mut album_fetch_requests = Vec::new();

    for (artist_id, artist_result) in artist_fetches {
        let Some(artist_summary) = artist_summaries_by_id.remove(&artist_id) else {
            continue;
        };
        let artist_albums = match artist_result {
            Ok(albums) => albums,
            Err(e) => {
                had_fetch_errors = true;
                warn!("Error fetching artist {artist_id} during full reconcile: {e}");
                continue;
            }
        };

        artists_data.push(ArtistData {
            id: artist_id.clone(),
            name: artist_summary.name,
            album_count: artist_summary
                .album_count
                .max(usize_to_i32_saturating(artist_albums.len())),
            cover_art: artist_summary.cover_art,
        });

        for album in artist_albums {
            let album_id = album.id.clone();

            albums_data.push(AlbumData {
                id: album_id.clone(),
                artist_id: artist_id.clone(),
                name: album.name,
                year: album.year,
                song_count: album.song_count,
                duration: Some(album.duration),
                cover_art: album.cover_art,
            });
            album_fetch_requests.push(AlbumFetchRequest {
                album_id,
                artist_id: artist_id.clone(),
                album_year: album.year,
            });
        }
    }

    let mut album_fetches = fetch_album_songs_bounded(
        state.client.clone(),
        album_fetch_requests,
        ALBUM_FETCH_CONCURRENCY,
    )
    .await;
    album_fetches.sort_by(|left, right| left.0.album_id.cmp(&right.0.album_id));

    for (request, album_result) in album_fetches {
        match album_result {
            Ok(songs) => {
                for song in songs {
                    songs_data.push(SongData {
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
                    });
                }
            }
            Err(e) => {
                had_fetch_errors = true;
                warn!(
                    "Error fetching album {} during full reconcile: {}",
                    request.album_id, e
                );
            }
        }
    }

    let newest_head_album_id = match state.client.get_newest_albums(1, 0).await {
        Ok(newest) => newest.first().map(|album| album.id.clone()),
        Err(e) => {
            warn!("Failed to fetch newest head album during full reconcile: {e}");
            None
        }
    };

    info!(
        "Applying full library reconcile (upserts: {} artists, {} albums, {} songs, prune_stale={})",
        artists_data.len(),
        albums_data.len(),
        songs_data.len(),
        !had_fetch_errors
    );

    let now = Utc::now().to_rfc3339();
    let db = state.db.lock_recover();
    db.execute("BEGIN IMMEDIATE", [])?;

    let result = (|| {
        for artist in &artists_data {
            db.execute(
                "INSERT OR REPLACE INTO artists (id, name, album_count, cover_art_id, synced_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![artist.id, artist.name, artist.album_count, artist.cover_art, &now],
            )?;
        }

        for album in &albums_data {
            db.execute(
                "INSERT OR REPLACE INTO albums (id, artist_id, name, year, song_count, duration, cover_art_id, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![album.id, album.artist_id, album.name, album.year, album.song_count, album.duration, album.cover_art, &now],
            )?;
        }

        for song in &songs_data {
            db.execute(
                "INSERT OR REPLACE INTO songs (id, album_id, artist_id, title, track_number, disc_number, duration, bit_rate, size, suffix, content_type, path, year, genre, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                rusqlite::params![song.id, song.album_id, song.artist_id, song.title, song.track, song.disc_number, song.duration, song.bit_rate, song.size, song.suffix, song.content_type, song.path, song.year, song.genre, &now],
            )?;
        }

        if had_fetch_errors {
            warn!(
                "Full reconcile completed with fetch errors, skipping stale-row deletion for safety"
            );
        } else {
            prune_stale_library_rows(&db, &now)?;
        }

        if let Some(head_album_id) = newest_head_album_id.as_deref() {
            set_sync_state(&db, NEWEST_HEAD_ALBUM_KEY, head_album_id, &now)?;
        }

        Ok::<(), crate::error::AppError>(())
    })();

    match result {
        Ok(()) => db.execute("COMMIT", [])?,
        Err(e) => {
            let _ = db.execute("ROLLBACK", []);
            return Err(e);
        }
    };

    drop(db);
    rebuild_search_index_from_db(state)?;

    let incoming_artist_ids: HashSet<String> = artists_data
        .iter()
        .map(|artist| artist.id.clone())
        .collect();
    let incoming_album_ids: HashSet<String> =
        albums_data.iter().map(|album| album.id.clone()).collect();
    let incoming_song_ids: HashSet<String> =
        songs_data.iter().map(|song| song.id.clone()).collect();
    let new_item_counts = count_full_reconcile_new_items(
        &local_artist_ids,
        &local_album_ids,
        &local_song_ids,
        &incoming_artist_ids,
        &incoming_album_ids,
        &incoming_song_ids,
    );

    Ok((
        SyncResult {
            artists: artists_data.len(),
            albums: albums_data.len(),
            songs: songs_data.len(),
        },
        new_item_counts,
    ))
}

async fn fetch_newest_album_candidates(
    state: &AppState,
    previous_head_album_id: Option<&str>,
    known_album_ids: &HashSet<String>,
) -> AppResult<NewestScanResult> {
    let mut head_album_id = None;
    let mut candidates: Vec<NewestAlbumCandidate> = Vec::new();
    let mut offset = 0usize;
    let mut stop_reason = NewestScanStopReason::ExhaustedNewestFeed;

    loop {
        let page = state
            .client
            .get_newest_albums(NEWEST_ALBUMS_PAGE_SIZE, offset)
            .await
            .map_err(|e| AppError::Subsonic(e.to_string()))?
            .into_iter()
            .map(|album| NewestAlbumPageEntry {
                id: album.id,
                artist_id: album.artist_id,
                artist_name: album.artist_name,
            })
            .collect::<Vec<_>>();

        if page.is_empty() {
            break;
        }

        if head_album_id.is_none() {
            head_album_id = Some(page[0].id.clone());
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

        offset += page_len;
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

async fn fetch_artist_albums_bounded(
    client: crate::client::SubsonicClientHandle,
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
        spawn_artist_fetch(&mut join_set, &client, artist_id);
    }

    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok(result) => results.push(result),
            Err(e) => {
                warn!("Artist fetch task failed: {e}");
            }
        }

        if let Some(artist_id) = pending_artist_ids.next() {
            spawn_artist_fetch(&mut join_set, &client, artist_id);
        }
    }

    results
}

fn spawn_artist_fetch(
    join_set: &mut JoinSet<(String, Result<Vec<RemoteAlbumSummary>, String>)>,
    client: &crate::client::SubsonicClientHandle,
    artist_id: String,
) {
    let client = client.clone();
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
    client: crate::client::SubsonicClientHandle,
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
        spawn_album_fetch(&mut join_set, &client, request);
    }

    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok(result) => results.push(result),
            Err(e) => {
                warn!("Album fetch task failed: {e}");
            }
        }

        if let Some(request) = pending_requests.next() {
            spawn_album_fetch(&mut join_set, &client, request);
        }
    }

    results
}

fn spawn_album_fetch(
    join_set: &mut JoinSet<(AlbumFetchRequest, Result<Vec<RemoteSong>, String>)>,
    client: &crate::client::SubsonicClientHandle,
    request: AlbumFetchRequest,
) {
    let client = client.clone();
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

fn load_local_artists(conn: &Connection) -> AppResult<HashMap<String, LocalArtistRow>> {
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

fn load_local_artist_ids(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM artists")?;

    let mut artist_ids: HashSet<String> = HashSet::new();

    for row in stmt.query_map([], |row| row.get::<_, String>(0))? {
        artist_ids.insert(row?);
    }

    Ok(artist_ids)
}

#[cfg(test)]
mod tests {
    use super::{
        NewItemCounts, NewestAlbumCandidate, NewestAlbumPageEntry, NewestPageScanResult,
        count_full_reconcile_new_items, count_incremental_new_items, prune_stale_library_rows,
        scan_newest_album_page,
    };
    use rusqlite::Connection;
    use std::collections::HashSet;

    fn album(id: &str, artist_id: Option<&str>, artist_name: Option<&str>) -> NewestAlbumPageEntry {
        NewestAlbumPageEntry {
            id: id.to_string(),
            artist_id: artist_id.map(str::to_string),
            artist_name: artist_name.map(str::to_string),
        }
    }

    fn known_album_ids(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    fn ids(values: &[&str]) -> HashSet<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        conn.execute_batch(include_str!("../db/schema.sql"))
            .expect("apply schema");
        conn
    }

    #[test]
    fn imports_unknown_albums_before_previous_head_even_after_known_album() {
        let page = vec![
            album("known-1", Some("artist-known"), Some("Known Artist")),
            album("new-1", Some("artist-new"), Some("New Artist")),
            album("prev-head", Some("artist-prev"), Some("Prev Artist")),
            album("new-2", Some("artist-late"), Some("Late Artist")),
        ];

        let result =
            scan_newest_album_page(&page, Some("prev-head"), &known_album_ids(&["known-1"]));

        assert_eq!(
            result.candidates,
            vec![NewestAlbumCandidate {
                album_id: "new-1".to_string(),
                artist_id: "artist-new".to_string(),
                artist_name: Some("New Artist".to_string()),
            }]
        );
        assert!(result.reached_previous_head);
    }

    #[test]
    fn known_albums_do_not_stop_scan_before_previous_head() {
        let page = vec![
            album("known-1", Some("artist-known"), Some("Known Artist")),
            album("known-2", Some("artist-known"), Some("Known Artist")),
            album("new-1", Some("artist-new"), Some("New Artist")),
            album("prev-head", Some("artist-prev"), Some("Prev Artist")),
        ];

        let result = scan_newest_album_page(
            &page,
            Some("prev-head"),
            &known_album_ids(&["known-1", "known-2"]),
        );

        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].album_id, "new-1");
        assert!(result.reached_previous_head);
    }

    #[test]
    fn stops_collecting_once_previous_head_is_encountered() {
        let page = vec![
            album("new-1", Some("artist-new"), Some("New Artist")),
            album("prev-head", Some("artist-prev"), Some("Prev Artist")),
            album("new-2", Some("artist-late"), Some("Late Artist")),
        ];

        let result = scan_newest_album_page(&page, Some("prev-head"), &known_album_ids(&[]));

        assert_eq!(result.candidates.len(), 1);
        assert_eq!(result.candidates[0].album_id, "new-1");
        assert!(result.reached_previous_head);
    }

    #[test]
    fn without_previous_head_it_collects_unknown_albums_and_never_stops_early() {
        let page = vec![
            album("known-1", Some("artist-known"), Some("Known Artist")),
            album("new-1", Some("artist-new"), Some("New Artist")),
            album("new-2", Some("artist-new-2"), Some("New Artist 2")),
        ];

        let result = scan_newest_album_page(&page, None, &known_album_ids(&["known-1"]));

        assert_eq!(
            result.candidates,
            vec![
                NewestAlbumCandidate {
                    album_id: "new-1".to_string(),
                    artist_id: "artist-new".to_string(),
                    artist_name: Some("New Artist".to_string()),
                },
                NewestAlbumCandidate {
                    album_id: "new-2".to_string(),
                    artist_id: "artist-new-2".to_string(),
                    artist_name: Some("New Artist 2".to_string()),
                },
            ]
        );
        assert!(!result.reached_previous_head);
    }

    #[test]
    fn skips_albums_without_artist_id_without_aborting_scan() {
        let page = vec![
            album("missing-artist", None, Some("Unknown")),
            album("new-1", Some("artist-new"), Some("New Artist")),
            album("prev-head", Some("artist-prev"), Some("Prev Artist")),
        ];

        let result = scan_newest_album_page(&page, Some("prev-head"), &known_album_ids(&[]));

        assert_eq!(
            result,
            NewestPageScanResult {
                candidates: vec![NewestAlbumCandidate {
                    album_id: "new-1".to_string(),
                    artist_id: "artist-new".to_string(),
                    artist_name: Some("New Artist".to_string()),
                }],
                reached_previous_head: true,
            }
        );
    }

    #[test]
    fn incremental_new_item_counts_are_zero_when_no_unknown_albums_are_imported() {
        let counts = count_incremental_new_items(&ids(&["artist-1"]), &HashSet::new(), 0, 0);

        assert_eq!(counts, NewItemCounts::default());
    }

    #[test]
    fn incremental_new_item_counts_detect_new_album_for_existing_artist() {
        let counts = count_incremental_new_items(&ids(&["artist-1"]), &ids(&["artist-1"]), 1, 8);

        assert_eq!(
            counts,
            NewItemCounts {
                artists: 0,
                albums: 1,
                songs: 8,
            }
        );
    }

    #[test]
    fn incremental_new_item_counts_detect_new_artist() {
        let counts = count_incremental_new_items(
            &ids(&["artist-existing"]),
            &ids(&["artist-existing", "artist-new"]),
            2,
            15,
        );

        assert_eq!(
            counts,
            NewItemCounts {
                artists: 1,
                albums: 2,
                songs: 15,
            }
        );
    }

    #[test]
    fn full_reconcile_new_item_counts_are_zero_when_ids_are_unchanged() {
        let counts = count_full_reconcile_new_items(
            &ids(&["artist-1"]),
            &ids(&["album-1"]),
            &ids(&["song-1", "song-2"]),
            &ids(&["artist-1"]),
            &ids(&["album-1"]),
            &ids(&["song-1", "song-2"]),
        );

        assert_eq!(counts, NewItemCounts::default());
    }

    #[test]
    fn full_reconcile_new_item_counts_detect_new_ids() {
        let counts = count_full_reconcile_new_items(
            &ids(&["artist-1"]),
            &ids(&["album-1"]),
            &ids(&["song-1"]),
            &ids(&["artist-1", "artist-2"]),
            &ids(&["album-1", "album-2", "album-3"]),
            &ids(&["song-1", "song-2", "song-3"]),
        );

        assert_eq!(
            counts,
            NewItemCounts {
                artists: 1,
                albums: 2,
                songs: 2,
            }
        );
    }

    #[test]
    fn prune_stale_library_rows_removes_song_dependents_before_deleting_songs() {
        let conn = test_conn();

        conn.execute(
            "INSERT INTO artists (id, name, synced_at) VALUES (?1, ?2, ?3)",
            rusqlite::params!["artist-stale", "Stale Artist", "old"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["album-stale", "artist-stale", "Stale Album", "old"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, synced_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["song-stale", "album-stale", "artist-stale", "Stale Song", "old"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO playlists (id, name, created_at, changed_at, synced_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["playlist-1", "Playlist", "now", "now", "now"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO playlist_songs (playlist_id, song_id, position) VALUES (?1, ?2, ?3)",
            rusqlite::params!["playlist-1", "song-stale", 0],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO normalization_data (song_id, track_loudness_lufs, track_peak, album_id, analyzed_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["song-stale", -14.0_f64, 0.9_f64, "album-stale", "now"],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO artists (id, name, synced_at) VALUES (?1, ?2, ?3)",
            rusqlite::params!["artist-keep", "Keep Artist", "now"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO albums (id, artist_id, name, synced_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params!["album-keep", "artist-keep", "Keep Album", "now"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO songs (id, album_id, artist_id, title, synced_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params!["song-keep", "album-keep", "artist-keep", "Keep Song", "now"],
        )
        .unwrap();

        prune_stale_library_rows(&conn, "now").unwrap();

        let playlist_song_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM playlist_songs", [], |row| row.get(0))
            .unwrap();
        let normalization_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM normalization_data", [], |row| {
                row.get(0)
            })
            .unwrap();
        let stale_song_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM songs WHERE id = 'song-stale'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let kept_song_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM songs WHERE id = 'song-keep'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(playlist_song_count, 0);
        assert_eq!(normalization_count, 0);
        assert_eq!(stale_song_count, 0);
        assert_eq!(kept_song_count, 1);
    }
}

fn load_local_album_ids(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM albums")?;

    let mut album_ids: HashSet<String> = HashSet::new();

    for row in stmt.query_map([], |row| row.get::<_, String>(0))? {
        album_ids.insert(row?);
    }

    Ok(album_ids)
}

fn load_local_song_ids(conn: &Connection) -> AppResult<HashSet<String>> {
    let mut stmt = conn.prepare("SELECT id FROM songs")?;

    let mut song_ids: HashSet<String> = HashSet::new();

    for row in stmt.query_map([], |row| row.get::<_, String>(0))? {
        song_ids.insert(row?);
    }

    Ok(song_ids)
}

fn count_incremental_new_items(
    local_artist_ids: &HashSet<String>,
    refreshed_artist_ids: &HashSet<String>,
    new_album_count: usize,
    new_song_count: usize,
) -> NewItemCounts {
    NewItemCounts {
        artists: refreshed_artist_ids
            .iter()
            .filter(|artist_id| !local_artist_ids.contains(*artist_id))
            .count(),
        albums: new_album_count,
        songs: new_song_count,
    }
}

fn count_full_reconcile_new_items(
    local_artist_ids: &HashSet<String>,
    local_album_ids: &HashSet<String>,
    local_song_ids: &HashSet<String>,
    incoming_artist_ids: &HashSet<String>,
    incoming_album_ids: &HashSet<String>,
    incoming_song_ids: &HashSet<String>,
) -> NewItemCounts {
    NewItemCounts {
        artists: incoming_artist_ids
            .iter()
            .filter(|artist_id| !local_artist_ids.contains(*artist_id))
            .count(),
        albums: incoming_album_ids
            .iter()
            .filter(|album_id| !local_album_ids.contains(*album_id))
            .count(),
        songs: incoming_song_ids
            .iter()
            .filter(|song_id| !local_song_ids.contains(*song_id))
            .count(),
    }
}

fn prune_stale_library_rows(conn: &Connection, synced_at: &str) -> AppResult<()> {
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

fn get_sync_state(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    conn.query_row(
        "SELECT value FROM sync_state WHERE key = ?1",
        [key],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn set_sync_state(conn: &Connection, key: &str, value: &str, updated_at: &str) -> AppResult<()> {
    conn.execute(
        "INSERT INTO sync_state (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![key, value, updated_at],
    )?;

    Ok(())
}

fn clear_sync_state(conn: &Connection, key: &str) -> AppResult<()> {
    conn.execute("DELETE FROM sync_state WHERE key = ?1", [key])?;
    Ok(())
}

fn parse_rfc3339_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
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

    Some(
        (last_attempt + ChronoDuration::minutes(i64::from(interval_minutes)))
            .with_timezone(&Utc)
            .to_rfc3339(),
    )
}

fn next_due_sync_job(
    state: &AppState,
    settings: &crate::commands::settings::SyncSettings,
) -> AppResult<Option<SyncJobKind>> {
    let now = Utc::now();
    let db = state.db.lock_recover();

    let full_last_attempt = get_sync_state(&db, FULL_LAST_ATTEMPT_AT_KEY)?;
    let incremental_last_attempt = get_sync_state(&db, INCREMENTAL_LAST_ATTEMPT_AT_KEY)?;

    if is_job_due(
        settings.full_reconcile_enabled,
        settings.full_reconcile_interval_hours.saturating_mul(60),
        full_last_attempt.as_deref(),
        now,
    ) {
        return Ok(Some(SyncJobKind::FullReconcile));
    }

    if is_job_due(
        settings.incremental_enabled,
        settings.incremental_interval_minutes,
        incremental_last_attempt.as_deref(),
        now,
    ) {
        return Ok(Some(SyncJobKind::Incremental));
    }

    Ok(None)
}

fn read_library_sync_status(
    state: &AppState,
    settings: &crate::commands::settings::SyncSettings,
) -> AppResult<LibrarySyncStatus> {
    let now = Utc::now();
    let active_job = *active_sync_job_state().lock_recover();

    let db = state.db.lock_recover();
    let incremental_last_attempt = get_sync_state(&db, INCREMENTAL_LAST_ATTEMPT_AT_KEY)?;
    let incremental_last_success = get_sync_state(&db, INCREMENTAL_LAST_SUCCESS_AT_KEY)?;
    let incremental_last_error = get_sync_state(&db, INCREMENTAL_LAST_ERROR_KEY)?;

    let full_last_attempt = get_sync_state(&db, FULL_LAST_ATTEMPT_AT_KEY)?;
    let full_last_success = get_sync_state(&db, FULL_LAST_SUCCESS_AT_KEY)?;
    let full_last_error = get_sync_state(&db, FULL_LAST_ERROR_KEY)?;

    let incremental = SyncJobStatus {
        enabled: settings.incremental_enabled,
        interval_minutes: settings.incremental_interval_minutes,
        running: active_job == Some(SyncJobKind::Incremental),
        next_run_at: compute_next_run_at(
            settings.incremental_enabled,
            settings.incremental_interval_minutes,
            incremental_last_attempt.as_deref(),
            now,
        ),
        last_attempt_at: incremental_last_attempt,
        last_success_at: incremental_last_success,
        last_error: incremental_last_error,
    };

    let full_reconcile_interval_minutes = settings.full_reconcile_interval_hours.saturating_mul(60);
    let full_reconcile = SyncJobStatus {
        enabled: settings.full_reconcile_enabled,
        interval_minutes: full_reconcile_interval_minutes,
        running: active_job == Some(SyncJobKind::FullReconcile),
        next_run_at: compute_next_run_at(
            settings.full_reconcile_enabled,
            full_reconcile_interval_minutes,
            full_last_attempt.as_deref(),
            now,
        ),
        last_attempt_at: full_last_attempt,
        last_success_at: full_last_success,
        last_error: full_last_error,
    };

    Ok(LibrarySyncStatus {
        active_job,
        incremental,
        full_reconcile,
    })
}

fn emit_library_sync_status_changed(state: &AppState, app_handle: &AppHandle) {
    let settings = crate::commands::settings::read_sync_settings(app_handle);
    match read_library_sync_status(state, &settings) {
        Ok(status) => {
            let _ = app_handle.emit("library-sync-status-changed", &status);
        }
        Err(e) => {
            warn!("Failed to emit library sync status event: {e}");
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn emit_library_content_updated(app_handle: &AppHandle, event: LibraryContentUpdatedEvent) {
    if !event.has_new_items {
        return;
    }

    let _ = app_handle.emit("library-content-updated", &event);
}

#[allow(clippy::unnecessary_wraps)]
fn update_search_index_for_incremental_sync(
    state: &AppState,
    artists: &[ArtistData],
    albums: &[AlbumData],
    songs: &[SongData],
) -> AppResult<()> {
    let artist_names_by_id: HashMap<&str, &str> = artists
        .iter()
        .map(|artist| (artist.id.as_str(), artist.name.as_str()))
        .collect();
    let album_names_by_id: HashMap<&str, &str> = albums
        .iter()
        .map(|album| (album.id.as_str(), album.name.as_str()))
        .collect();

    let artist_docs: Vec<ArtistIndexData> = artists
        .iter()
        .map(|artist| ArtistIndexData {
            id: artist.id.clone(),
            name: artist.name.clone(),
            album_count: artist.album_count,
        })
        .collect();

    let album_docs: Vec<AlbumIndexData> = albums
        .iter()
        .filter_map(|album| {
            let artist_name = artist_names_by_id.get(album.artist_id.as_str())?;
            Some(AlbumIndexData {
                id: album.id.clone(),
                name: album.name.clone(),
                artist_name: (*artist_name).to_string(),
                year: album.year,
                song_count: album.song_count,
            })
        })
        .collect();

    let song_docs: Vec<SongIndexData> = songs
        .iter()
        .filter_map(|song| {
            let artist_name = artist_names_by_id.get(song.artist_id.as_str())?;
            let album_name = album_names_by_id.get(song.album_id.as_str())?;
            Some(SongIndexData {
                id: song.id.clone(),
                title: song.title.clone(),
                artist_name: (*artist_name).to_string(),
                album_name: (*album_name).to_string(),
                genre: song.genre.clone(),
                year: song.year,
                duration: song.duration,
            })
        })
        .collect();

    if album_docs.len() != albums.len() || song_docs.len() != songs.len() {
        warn!(
            "Incremental search update skipped {} albums and {} songs due to missing delta metadata",
            albums.len().saturating_sub(album_docs.len()),
            songs.len().saturating_sub(song_docs.len())
        );
    }

    let mut search_index_guard = state.search_index.lock_recover();

    if search_index_guard.is_none() {
        debug!("Search index is None, creating new one...");
        match IndexManager::new(&state.index_path) {
            Ok(manager) => {
                info!("Created new search index");
                *search_index_guard = Some(manager);
            }
            Err(e) => {
                warn!("Failed to create search index: {e}");
                return Ok(());
            }
        }
    }

    if let Some(ref index_manager) = *search_index_guard
        && let Err(e) = index_manager.upsert_documents(&artist_docs, &album_docs, &song_docs)
    {
        warn!("Failed to incrementally update search index: {e}");
    }

    Ok(())
}

/// Rebuild the search index from current database state.
fn rebuild_search_index_from_db(state: &AppState) -> AppResult<()> {
    let (artists, albums, songs) = {
        let db = state.db.lock_recover();

        let mut artists_stmt = db.prepare("SELECT id, name, album_count FROM artists")?;
        let artists: Vec<ArtistIndexData> = artists_stmt
            .query_map([], |row| {
                Ok(ArtistIndexData {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    album_count: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut albums_stmt = db.prepare(
            "SELECT al.id, al.name, ar.name, al.year, al.song_count
             FROM albums al
             LEFT JOIN artists ar ON ar.id = al.artist_id",
        )?;
        let albums: Vec<AlbumIndexData> = albums_stmt
            .query_map([], |row| {
                Ok(AlbumIndexData {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    artist_name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    year: row.get(3)?,
                    song_count: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut songs_stmt = db.prepare(
            "SELECT s.id, s.title, ar.name, al.name, s.genre, s.year, s.duration
             FROM songs s
             LEFT JOIN artists ar ON ar.id = s.artist_id
             LEFT JOIN albums al ON al.id = s.album_id",
        )?;
        let songs: Vec<SongIndexData> = songs_stmt
            .query_map([], |row| {
                Ok(SongIndexData {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    artist_name: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    album_name: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    genre: row.get(4)?,
                    year: row.get(5)?,
                    duration: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        (artists, albums, songs)
    };

    debug!(
        "rebuild_search_index_from_db called with {} artists, {} albums, {} songs",
        artists.len(),
        albums.len(),
        songs.len()
    );

    // Get or create the search index.
    let mut search_index_guard = state.search_index.lock_recover();

    if search_index_guard.is_none() {
        debug!("Search index is None, creating new one...");
        match IndexManager::new(&state.index_path) {
            Ok(manager) => {
                info!("Created new search index");
                *search_index_guard = Some(manager);
            }
            Err(e) => {
                warn!("Failed to create search index: {e}");
                return Ok(()); // Don't fail the sync.
            }
        }
    }

    if let Some(ref index_manager) = *search_index_guard
        && let Err(e) = index_manager.rebuild_index(&artists, &albums, &songs)
    {
        warn!("Failed to rebuild search index: {e}");
    }

    Ok(())
}

#[tauri::command]
pub async fn get_artists(state: State<'_, AppState>) -> AppResult<Vec<Artist>> {
    let db = state.db.lock_recover();
    let mut stmt = db.prepare(
        "SELECT id, name, album_count, cover_art_id, synced_at FROM artists ORDER BY name",
    )?;

    let artists: Vec<Artist> = stmt
        .query_map([], |row| {
            Ok(Artist {
                id: row.get(0)?,
                name: row.get(1)?,
                album_count: row.get(2)?,
                cover_art_id: row.get(3)?,
                synced_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(artists)
}

#[tauri::command]
#[allow(clippy::needless_pass_by_value)]
pub fn get_album_count(state: State<'_, AppState>) -> AppResult<i64> {
    let db = state.db.lock_recover();
    let count: i64 = db.query_row("SELECT COUNT(*) FROM albums", [], |row| row.get(0))?;
    Ok(count)
}

#[tauri::command]
pub async fn get_albums(
    state: State<'_, AppState>,
    artist_id: Option<String>,
) -> AppResult<Vec<Album>> {
    let db = state.db.lock_recover();

    let albums: Vec<Album> = if let Some(aid) = artist_id {
        let mut stmt = db.prepare(
            "SELECT al.id, al.artist_id, al.name, al.year, al.song_count, al.duration, al.cover_art_id, al.synced_at, ar.name as artist_name
             FROM albums al
             LEFT JOIN artists ar ON al.artist_id = ar.id
             WHERE al.artist_id = ?1
             ORDER BY al.year, al.name",
        )?;
        let result: Vec<Album> = stmt
            .query_map([aid], |row| {
                Ok(Album {
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
            })?
            .collect::<Result<Vec<_>, _>>()?;
        result
    } else {
        let mut stmt = db.prepare(
            "SELECT al.id, al.artist_id, al.name, al.year, al.song_count, al.duration, al.cover_art_id, al.synced_at, ar.name as artist_name
             FROM albums al
             LEFT JOIN artists ar ON al.artist_id = ar.id
             ORDER BY al.name",
        )?;
        let result: Vec<Album> = stmt
            .query_map([], |row| {
                Ok(Album {
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
            })?
            .collect::<Result<Vec<_>, _>>()?;
        result
    };

    Ok(albums)
}

#[tauri::command]
pub async fn get_songs(
    state: State<'_, AppState>,
    album_id: Option<String>,
    artist_id: Option<String>,
) -> AppResult<Vec<Song>> {
    let db = state.db.lock_recover();

    let base_query = "SELECT s.id, s.album_id, s.artist_id, s.title, s.track_number, s.disc_number,
                      s.duration, s.bit_rate, s.size, s.suffix, s.content_type, s.path,
                      s.year, s.genre, s.synced_at, ar.name as artist_name, al.name as album_name
                      FROM songs s
                      LEFT JOIN artists ar ON s.artist_id = ar.id
                      LEFT JOIN albums al ON s.album_id = al.id";

    let map_row = |row: &rusqlite::Row| -> rusqlite::Result<Song> {
        Ok(Song {
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
    };

    let songs: Vec<Song> = if let Some(aid) = album_id {
        let query =
            format!("{base_query} WHERE s.album_id = ?1 ORDER BY s.disc_number, s.track_number");
        let mut stmt = db.prepare(&query)?;
        let result: Vec<Song> = stmt
            .query_map([aid], map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        result
    } else if let Some(aid) = artist_id {
        let query = format!(
            "{base_query} WHERE s.artist_id = ?1 ORDER BY al.name COLLATE NOCASE, s.disc_number, s.track_number"
        );
        let mut stmt = db.prepare(&query)?;
        let result: Vec<Song> = stmt
            .query_map([aid], map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        result
    } else {
        let query = format!(
            "{base_query} ORDER BY ar.name COLLATE NOCASE, al.name COLLATE NOCASE, s.disc_number, s.track_number"
        );
        let mut stmt = db.prepare(&query)?;
        let result: Vec<Song> = stmt
            .query_map([], map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        result
    };

    Ok(songs)
}

#[tauri::command]
pub async fn get_album_list(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    list_type: String,
    size: Option<u32>,
    offset: Option<u32>,
) -> AppResult<Vec<crate::client::AlbumListEntry>> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(AppError::OfflineMode);
    }

    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    let order = crate::client::AlbumListOrder::from_str(&list_type)
        .ok_or_else(|| AppError::Subsonic(format!("Unknown album list type: {list_type}")))?;

    let size = size.unwrap_or(40) as usize;
    let offset = offset.unwrap_or(0) as usize;

    state
        .client
        .get_album_list(order, size, offset)
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))
}

#[tauri::command]
pub async fn get_scan_status(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ScanStatus> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(AppError::OfflineMode);
    }

    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    let status = state
        .client
        .get_scan_status()
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    Ok(ScanStatus {
        scanning: status.scanning,
        count: status.count,
    })
}

#[tauri::command]
pub async fn start_scan(
    app_handle: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ScanStatus> {
    if crate::commands::settings::manual_offline_enabled(&app_handle) {
        return Err(AppError::OfflineMode);
    }

    if !state.client.is_connected() {
        return Err(AppError::NotConnected);
    }

    let status = state
        .client
        .start_scan()
        .await
        .map_err(|e| AppError::Subsonic(e.to_string()))?;

    Ok(ScanStatus {
        scanning: status.scanning,
        count: status.count,
    })
}
