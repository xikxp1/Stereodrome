mod db;
mod error;
mod models;
pub mod queue;
mod subsonic;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use log::{debug, info, warn};
use queue::{PlayQueue, QueueItem, QueueState, RepeatMode};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use submarine::{Client, api::get_album_list::Order, auth::AuthBuilder};
use tokio::sync::Mutex as AsyncMutex;

pub use error::{CoreError, CoreResult};
pub use models::*;
pub use queue::{QueueItem as SharedQueueItem, QueueState as SharedQueueState};

const API_VERSION: &str = "1.16.1";
const CLIENT_NAME: &str = "StereodromeMobile";
const MOBILE_PLAYBACK_FORMAT: &str = "mp3";
const LARGE_COVER_ART_SIZE: i32 = 512;

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

#[derive(Debug)]
pub struct StereodromeCore {
    data_dir: PathBuf,
    db_path: PathBuf,
    config_path: PathBuf,
    server_config: Mutex<Option<ServerConfig>>,
    client: AsyncMutex<Option<Client>>,
    queue: Mutex<PlayQueue>,
}

impl StereodromeCore {
    pub fn new(data_dir: impl AsRef<Path>) -> CoreResult<Self> {
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
        })
    }

    pub async fn connect_server(&self, params: ConnectParams) -> CoreResult<ConnectionStatus> {
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

    pub async fn restore_session(&self) -> CoreResult<ConnectionStatus> {
        info!("Restoring saved Subsonic session");
        let config = {
            self.server_config
                .lock()
                .map_err(|_| CoreError::LockPoisoned)?
                .clone()
        };

        let Some(config) = config else {
            debug!("No saved Subsonic session to restore");
            return Ok(ConnectionStatus::disconnected());
        };

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

    pub async fn sync_library(&self) -> CoreResult<SyncResult> {
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

        let now = Utc::now().to_rfc3339();
        let mut conn = Connection::open(&self.db_path)?;
        let tx = conn.transaction()?;

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO artists
                 (id, name, album_count, cover_art_id, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for artist in &sync_data.artists {
                stmt.execute(params![
                    artist.id,
                    artist.name,
                    artist.album_count,
                    artist.cover_art,
                    now
                ])?;
            }
        }

        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO albums
                 (id, artist_id, name, year, song_count, duration, cover_art_id, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for album in &sync_data.albums {
                stmt.execute(params![
                    album.id,
                    album.artist_id,
                    album.name,
                    album.year,
                    album.song_count,
                    album.duration,
                    album.cover_art,
                    now
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
            for song in &sync_data.songs {
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
                    now
                ])?;
            }
        }

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
        tx.commit()?;
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

    pub async fn sync_library_incremental(&self) -> CoreResult<SyncResult> {
        info!("Starting incremental library sync");
        self.record_sync_attempt("library_incremental", None)?;
        match self.sync_library().await {
            Ok(result) => {
                self.record_sync_success("library_incremental")?;
                info!(
                    "Incremental library sync complete: artists={}, albums={}, songs={}",
                    result.artists, result.albums, result.songs
                );
                Ok(result)
            }
            Err(error) => {
                warn!("Incremental library sync failed: {error}");
                self.record_sync_attempt("library_incremental", Some(error.to_string()))?;
                Err(error)
            }
        }
    }

    pub async fn reconcile_library(&self) -> CoreResult<SyncResult> {
        info!("Starting full library sync with reconciliation");
        self.record_sync_attempt("library_reconcile", None)?;

        let result = match self.sync_library().await {
            Ok(result) => {
                let conn = Connection::open(&self.db_path)?;
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
                self.record_sync_success("library_reconcile")?;
                info!(
                    "Full library sync with reconciliation complete: artists={}, albums={}, songs={}",
                    result.artists, result.albums, result.songs
                );
                Ok(result)
            }
            Err(error) => {
                warn!("Full library sync with reconciliation failed: {error}");
                self.record_sync_attempt("library_reconcile", Some(error.to_string()))?;
                Err(error)
            }
        }
    }

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

    pub fn get_library_sync_status(&self) -> CoreResult<LibrarySyncStatus> {
        let conn = Connection::open(&self.db_path)?;
        let full = self.sync_job_status(&conn, "library_full", false, 1440)?;
        let incremental = self.sync_job_status(&conn, "library_incremental", true, 60)?;
        let reconcile = self.sync_job_status(&conn, "library_reconcile", false, 1440)?;

        Ok(LibrarySyncStatus {
            active_job: None,
            full,
            incremental,
            full_reconcile: reconcile,
        })
    }

    pub fn get_artists(&self) -> CoreResult<Vec<Artist>> {
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, name, album_count, cover_art_id, synced_at
             FROM artists ORDER BY name COLLATE NOCASE",
        )?;
        rows_collect(stmt.query_map([], Artist::from_row)?)
    }

    pub fn get_albums(&self, artist_id: Option<String>) -> CoreResult<Vec<Album>> {
        let conn = Connection::open(&self.db_path)?;
        match artist_id {
            Some(artist_id) => {
                let mut stmt = conn.prepare(
                    "SELECT al.id, al.artist_id, al.name, al.year, al.song_count, al.duration,
                            al.cover_art_id, al.synced_at, ar.name
                     FROM albums al
                     LEFT JOIN artists ar ON al.artist_id = ar.id
                     WHERE al.artist_id = ?1
                     ORDER BY COALESCE(al.year, 9999), al.name COLLATE NOCASE",
                )?;
                rows_collect(stmt.query_map([artist_id], Album::from_row)?)
            }
            None => {
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
    }

    pub fn get_songs(
        &self,
        album_id: Option<String>,
        artist_id: Option<String>,
    ) -> CoreResult<Vec<Song>> {
        let conn = Connection::open(&self.db_path)?;
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

    pub fn search_library(&self, query: String, limit: Option<usize>) -> CoreResult<SearchResults> {
        let conn = Connection::open(&self.db_path)?;
        let like = format!("%{}%", query);
        let limit = limit.unwrap_or(25).min(100) as i64;

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
            return Ok(mapped);
        }

        self.get_local_playlists()
    }

    pub async fn get_playlist_songs(&self, playlist_id: String) -> CoreResult<Vec<Song>> {
        let client = self.connected_client().await?;
        let playlist = client
            .get_playlist(&playlist_id)
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        let now = Utc::now().to_rfc3339();
        Ok(playlist
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
            .collect())
    }

    pub async fn create_playlist(
        &self,
        name: String,
        song_ids: Vec<String>,
    ) -> CoreResult<Playlist> {
        let client = self.connected_client().await?;
        let playlist = client
            .create_playlist(name, song_ids)
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        let mapped = playlist_from_subsonic(playlist.base);
        self.save_playlists(std::slice::from_ref(&mapped))?;
        Ok(mapped)
    }

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

        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "UPDATE playlists SET name = ?1, synced_at = ?2 WHERE id = ?3",
            params![name, Utc::now().to_rfc3339(), playlist_id],
        )?;
        Ok(())
    }

    pub async fn delete_playlist(&self, playlist_id: String) -> CoreResult<()> {
        let client = self.connected_client().await?;
        client
            .delete_playlist(playlist_id.clone())
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "DELETE FROM playlist_songs WHERE playlist_id = ?1",
            [&playlist_id],
        )?;
        conn.execute("DELETE FROM playlists WHERE id = ?1", [&playlist_id])?;
        Ok(())
    }

    pub async fn add_songs_to_playlist(
        &self,
        playlist_id: String,
        song_ids: Vec<String>,
    ) -> CoreResult<()> {
        let client = self.connected_client().await?;
        client
            .update_playlist(
                playlist_id,
                None::<String>,
                None::<String>,
                None,
                song_ids,
                Vec::new(),
            )
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        Ok(())
    }

    pub async fn remove_songs_from_playlist(
        &self,
        playlist_id: String,
        song_indexes: Vec<i64>,
    ) -> CoreResult<()> {
        let client = self.connected_client().await?;
        client
            .update_playlist(
                playlist_id,
                None::<String>,
                None::<String>,
                None,
                Vec::<String>::new(),
                song_indexes,
            )
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        Ok(())
    }

    pub fn get_stream_uri(&self, song_id: String) -> CoreResult<String> {
        if let Some(path) = self.cached_song_path(&song_id)? {
            return Ok(path_to_file_uri(&path));
        }

        let config = self.current_config()?;
        Ok(subsonic::signed_url(
            &config,
            "stream",
            &[("id", song_id.as_str()), ("format", MOBILE_PLAYBACK_FORMAT)],
        ))
    }

    pub async fn get_cover_art_uri(
        &self,
        cover_art_id: String,
        size: Option<i32>,
    ) -> CoreResult<String> {
        let path = self.get_or_cache_cover_art(&cover_art_id, size).await?;
        self.prefetch_large_cover_art_if_small(&cover_art_id, size);
        Ok(path_to_file_uri(&path))
    }

    pub async fn get_song_cover_art_uri(
        &self,
        song_id: String,
        size: Option<i32>,
    ) -> CoreResult<Option<String>> {
        let conn = Connection::open(&self.db_path)?;
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

    pub fn get_audio_cache_stats(&self) -> CoreResult<CacheStats> {
        let max_size = self.max_cache_size()?;
        let entries = self.audio_cache_entries()?;
        Ok(CacheStats {
            total_size: entries.iter().map(|(_, size)| *size).sum(),
            file_count: entries.len() as u64,
            max_size,
        })
    }

    pub fn get_offline_song_ids(&self) -> CoreResult<Vec<String>> {
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare("SELECT id FROM songs ORDER BY title COLLATE NOCASE")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let library_song_ids = rows.collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        drop(conn);

        let mut song_ids = Vec::new();
        for song_id in library_song_ids {
            if self.cached_song_path(&song_id)?.is_some() {
                song_ids.push(song_id);
            }
        }

        Ok(song_ids)
    }

    pub fn set_max_cache_size(&self, max_size: u64) -> CoreResult<CacheStats> {
        let max_size = max_size.clamp(500 * 1024 * 1024, 50 * 1024 * 1024 * 1024);
        self.set_setting("max_cache_size", &max_size.to_string())?;
        self.enforce_audio_cache_limit()?;
        self.get_audio_cache_stats()
    }

    pub fn clear_audio_cache(&self) -> CoreResult<CacheStats> {
        for (path, _) in self.audio_cache_entries()? {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let conn = Connection::open(&self.db_path)?;
        conn.execute("DELETE FROM download_items", [])?;
        self.get_audio_cache_stats()
    }

    pub fn is_song_cached(&self, song_id: String) -> CoreResult<DownloadStatus> {
        let path = self.cached_song_path(&song_id)?;
        let bytes = path
            .as_ref()
            .and_then(|path| path.metadata().ok())
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        Ok(DownloadStatus {
            song_id,
            cached: path.is_some(),
            path: path.as_ref().map(|path| path_to_file_uri(path)),
            bytes,
        })
    }

    pub async fn download_song(&self, song_id: String) -> CoreResult<DownloadStatus> {
        if let Some(path) = self.cached_song_path(&song_id)? {
            let bytes = path.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            return Ok(DownloadStatus {
                song_id,
                cached: true,
                path: Some(path_to_file_uri(&path)),
                bytes,
            });
        }

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
        match client
            .stream(
                song_id.clone(),
                None,
                Some(MOBILE_PLAYBACK_FORMAT),
                None,
                None::<String>,
                None,
                None,
            )
            .await
        {
            Ok(bytes) => {
                std::fs::write(&path, &bytes)?;
                self.record_download(DownloadRecord {
                    entity_type: "song",
                    entity_id: &song_id,
                    song_id: &song_id,
                    status: "downloaded",
                    path: Some(&path),
                    bytes: bytes.len() as u64,
                    error: None,
                })?;
                self.enforce_audio_cache_limit()?;
                Ok(DownloadStatus {
                    song_id,
                    cached: true,
                    path: Some(path_to_file_uri(&path)),
                    bytes: bytes.len() as u64,
                })
            }
            Err(error) => {
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
                Err(CoreError::Subsonic(error.to_string()))
            }
        }
    }

    pub fn remove_cached_song(&self, song_id: String) -> CoreResult<DownloadStatus> {
        if let Some(path) = self.cached_song_path(&song_id)? {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        let conn = Connection::open(&self.db_path)?;
        conn.execute("DELETE FROM download_items WHERE song_id = ?1", [&song_id])?;
        Ok(DownloadStatus {
            song_id,
            cached: false,
            path: None,
            bytes: 0,
        })
    }

    pub async fn download_album(&self, album_id: String) -> CoreResult<Vec<DownloadStatus>> {
        let songs = self.get_songs(Some(album_id), None)?;
        let mut statuses = Vec::with_capacity(songs.len());
        for song in songs {
            statuses.push(self.download_song(song.id).await?);
        }
        Ok(statuses)
    }

    pub async fn download_playlist(&self, playlist_id: String) -> CoreResult<Vec<DownloadStatus>> {
        let songs = self.get_playlist_songs(playlist_id).await?;
        let mut statuses = Vec::with_capacity(songs.len());
        for song in songs {
            statuses.push(self.download_song(song.id).await?);
        }
        Ok(statuses)
    }

    pub async fn prefetch_next(&self) -> CoreResult<Option<DownloadStatus>> {
        let next = {
            let mut queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
            queue.peek_next().cloned()
        };
        match next {
            Some(item) => self.download_song(item.song_id).await.map(Some),
            None => Ok(None),
        }
    }

    pub fn peek_next_queue_item(&self) -> CoreResult<Option<QueueItem>> {
        let mut queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
        Ok(queue.peek_next().cloned())
    }

    pub fn songs_are_gapless_eligible(
        &self,
        current_song_id: &str,
        next_song_id: &str,
    ) -> CoreResult<bool> {
        let conn = Connection::open(&self.db_path)?;
        let Some(current) = gapless_track_info(&conn, current_song_id)? else {
            return Ok(false);
        };
        let Some(next) = gapless_track_info(&conn, next_song_id)? else {
            return Ok(false);
        };

        if current.album_id != next.album_id {
            return Ok(false);
        }

        let same_disc_consecutive = current.disc_number == next.disc_number
            && next.track_number == current.track_number + 1;
        let next_disc_first_track =
            next.disc_number == current.disc_number + 1 && next.track_number == 1;
        Ok(same_disc_consecutive || next_disc_first_track)
    }

    pub fn get_playback_state(&self) -> CoreResult<PlaybackState> {
        let conn = Connection::open(&self.db_path)?;
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

    pub async fn report_playback_progress(
        &self,
        progress: PlaybackProgress,
    ) -> CoreResult<PlaybackState> {
        let previous = self.playback_markers()?;
        let now_playing_song_id =
            if previous.now_playing_song_id.as_deref() == Some(&progress.song_id) {
                previous.now_playing_song_id.clone()
            } else {
                if let Ok(client) = self.connected_client().await {
                    let _ = client
                        .scrobble(vec![(progress.song_id.clone(), None)], Some(false))
                        .await;
                }
                Some(progress.song_id.clone())
            };

        let should_submit = progress.duration_seconds > 0.0
            && progress.position_seconds / progress.duration_seconds >= 0.5
            && previous.scrobbled_song_id.as_deref() != Some(&progress.song_id);
        let scrobbled_song_id = if should_submit {
            if let Ok(client) = self.connected_client().await {
                let timestamp = chrono::Utc::now().timestamp_millis().max(0) as usize;
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

        self.save_playback_state(PlaybackStateWrite {
            song_id: Some(progress.song_id),
            position_seconds: progress.position_seconds,
            duration_seconds: progress.duration_seconds,
            was_playing: progress.is_playing,
            app_volume: previous.app_volume,
            now_playing_song_id,
            scrobbled_song_id,
        })
    }

    pub fn save_playback_position(&self, progress: PlaybackProgress) -> CoreResult<PlaybackState> {
        let previous = self.playback_markers()?;
        self.save_playback_state(PlaybackStateWrite {
            song_id: Some(progress.song_id),
            position_seconds: progress.position_seconds,
            duration_seconds: progress.duration_seconds,
            was_playing: progress.is_playing,
            app_volume: previous.app_volume,
            now_playing_song_id: previous.now_playing_song_id,
            scrobbled_song_id: previous.scrobbled_song_id,
        })
    }

    pub fn get_audio_processing_settings(&self) -> CoreResult<AudioProcessingSettings> {
        let conn = Connection::open(&self.db_path)?;
        let Some(json) = sync_value(&conn, "settings_audio_processing")? else {
            return Ok(AudioProcessingSettings::default());
        };
        let mut settings =
            serde_json::from_str::<AudioProcessingSettings>(&json).unwrap_or_default();
        clamp_audio_processing_settings(&mut settings);
        Ok(settings)
    }

    pub fn set_audio_processing_settings(
        &self,
        mut settings: AudioProcessingSettings,
    ) -> CoreResult<AudioProcessingSettings> {
        clamp_audio_processing_settings(&mut settings);
        let conn = Connection::open(&self.db_path)?;
        write_sync_value(
            &conn,
            "settings_audio_processing",
            &serde_json::to_string(&settings)?,
        )?;
        Ok(settings)
    }

    pub fn get_queue(&self) -> CoreResult<QueueState> {
        self.with_queue_state(|_| Ok(()))
    }

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

        self.with_queue_state(|queue| {
            *queue = PlayQueue::load(queue_items, Some(current_index), false, RepeatMode::Off);
            Ok(())
        })
    }

    pub fn add_to_queue(&self, item: QueueItem) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.add(item);
            Ok(())
        })
    }

    pub fn add_songs_to_queue(&self, items: Vec<QueueItem>) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.add_many(items);
            Ok(())
        })
    }

    pub fn insert_next(&self, item: QueueItem) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.insert_next(item);
            Ok(())
        })
    }

    pub fn insert_next_songs(&self, items: Vec<QueueItem>) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.insert_many_next(items);
            Ok(())
        })
    }

    pub fn remove_from_queue(&self, index: usize) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.remove(index);
            Ok(())
        })
    }

    pub fn clear_queue(&self) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.clear();
            Ok(())
        })
    }

    pub fn move_queue_item(&self, from: usize, to: usize) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.move_item(from, to);
            Ok(())
        })
    }

    pub fn play_queue_item(&self, index: usize) -> CoreResult<Option<QueueItem>> {
        self.with_queue_result(|queue| Ok(queue.set_current(index).cloned()))
            .map(|(item, _)| item)
    }

    pub fn play_next(&self, force: Option<bool>) -> CoreResult<Option<QueueItem>> {
        self.with_queue_result(|queue| Ok(queue.next(force.unwrap_or(false)).cloned()))
            .map(|(item, _)| item)
    }

    pub fn play_previous(&self) -> CoreResult<Option<QueueItem>> {
        self.with_queue_result(|queue| Ok(queue.previous().cloned()))
            .map(|(item, _)| item)
    }

    pub fn toggle_shuffle(&self) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.toggle_shuffle();
            Ok(())
        })
    }

    pub fn set_repeat_mode(&self, mode: RepeatMode) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.set_repeat_mode(mode);
            Ok(())
        })
    }

    pub fn cycle_repeat_mode(&self) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.cycle_repeat_mode();
            Ok(())
        })
    }

    pub fn reroll_next(&self) -> CoreResult<QueueState> {
        self.with_queue_state(|queue| {
            queue.reroll_next();
            Ok(())
        })
    }

    async fn connected_client(&self) -> CoreResult<Client> {
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

    fn get_local_playlists(&self) -> CoreResult<Vec<Playlist>> {
        let conn = Connection::open(&self.db_path)?;
        let mut stmt = conn.prepare(
            "SELECT id, name, song_count, duration, owner, cover_art_id, created_at, changed_at
             FROM playlists ORDER BY name COLLATE NOCASE",
        )?;
        rows_collect(stmt.query_map([], |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                song_count: row.get(2)?,
                duration: row.get(3)?,
                owner: row.get(4)?,
                cover_art_id: row.get(5)?,
                created_at: row.get(6)?,
                changed_at: row.get(7)?,
            })
        })?)
    }

    fn save_playlists(&self, playlists: &[Playlist]) -> CoreResult<()> {
        let mut conn = Connection::open(&self.db_path)?;
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        {
            let mut stmt = tx.prepare(
                "INSERT OR REPLACE INTO playlists
                 (id, name, song_count, duration, owner, cover_art_id, created_at, changed_at, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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

    fn sync_job_status(
        &self,
        conn: &Connection,
        prefix: &str,
        enabled: bool,
        interval_minutes: u32,
    ) -> CoreResult<SyncJobStatus> {
        let last_attempt_at = sync_value(conn, &format!("{prefix}_last_attempt_at"))?;
        let last_success_at = sync_value(conn, &format!("{prefix}_last_success_at"))?;
        let last_error =
            sync_value(conn, &format!("{prefix}_last_error"))?.filter(|e| !e.is_empty());
        let next_run_at = last_success_at.as_deref().and_then(|value| {
            chrono::DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|date| date + chrono::Duration::minutes(interval_minutes as i64))
                .map(|date| date.to_rfc3339())
        });

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
        let conn = Connection::open(&self.db_path)?;
        let now = Utc::now().to_rfc3339();
        write_sync_value(&conn, &format!("{prefix}_last_attempt_at"), &now)?;
        if let Some(error) = error {
            write_sync_value(&conn, &format!("{prefix}_last_error"), &error)?;
        }
        Ok(())
    }

    fn record_sync_success(&self, prefix: &str) -> CoreResult<()> {
        let conn = Connection::open(&self.db_path)?;
        let now = Utc::now().to_rfc3339();
        write_sync_value(&conn, &format!("{prefix}_last_success_at"), &now)?;
        write_sync_value(&conn, &format!("{prefix}_last_error"), "")?;
        Ok(())
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

        let conn = Connection::open(&self.db_path)?;
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
        let safe_id = sanitize_file_component(cover_art_id);
        let filename = match size {
            Some(size) => format!("{safe_id}_{size}.jpg"),
            None => format!("{safe_id}.jpg"),
        };
        Ok(self.cover_cache_dir()?.join(filename))
    }

    async fn get_or_cache_cover_art(
        &self,
        cover_art_id: &str,
        size: Option<i32>,
    ) -> CoreResult<PathBuf> {
        let path = self.cover_cache_path(cover_art_id, size)?;
        if path.exists() {
            return Ok(path);
        }

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
            self.record_download(DownloadRecord {
                entity_type: "song",
                entity_id: song_id,
                song_id,
                status: "downloaded",
                path: Some(&mp3_path),
                bytes: mp3_path.metadata().map(|m| m.len()).unwrap_or(0),
                error: None,
            })?;
            return Ok(Some(mp3_path));
        }

        let conn = Connection::open(&self.db_path)?;
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
                self.touch_download(song_id)?;
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
            if path.is_file() {
                entries.push((path, entry.metadata()?.len()));
            }
        }
        Ok(entries)
    }

    fn max_cache_size(&self) -> CoreResult<u64> {
        let conn = Connection::open(&self.db_path)?;
        let value = sync_value(&conn, "setting_max_cache_size")?
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(5 * 1024 * 1024 * 1024);
        Ok(value.clamp(500 * 1024 * 1024, 50 * 1024 * 1024 * 1024))
    }

    fn set_setting(&self, key: &str, value: &str) -> CoreResult<()> {
        let conn = Connection::open(&self.db_path)?;
        write_sync_value(&conn, &format!("setting_{key}"), value)
    }

    fn enforce_audio_cache_limit(&self) -> CoreResult<()> {
        let max_size = self.max_cache_size()?;
        let mut entries = self.audio_cache_entries()?;
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
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    total_size = total_size.saturating_sub(size);
                    let path_string = path.to_string_lossy().to_string();
                    let conn = Connection::open(&self.db_path)?;
                    conn.execute("DELETE FROM download_items WHERE path = ?1", [&path_string])?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }

        Ok(())
    }

    fn record_download(&self, record: DownloadRecord<'_>) -> CoreResult<()> {
        let conn = Connection::open(&self.db_path)?;
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
                record.bytes as i64,
                record.error,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    fn touch_download(&self, song_id: &str) -> CoreResult<()> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "UPDATE download_items SET updated_at = ?1 WHERE song_id = ?2",
            params![Utc::now().to_rfc3339(), song_id],
        )?;
        Ok(())
    }

    fn playback_markers(&self) -> CoreResult<PlaybackMarkers> {
        let conn = Connection::open(&self.db_path)?;
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

    fn save_playback_state(&self, state: PlaybackStateWrite) -> CoreResult<PlaybackState> {
        let conn = Connection::open(&self.db_path)?;
        let updated_at = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO playback_state
             (id, current_song_id, position_seconds, duration_seconds, was_playing, app_volume,
              now_playing_song_id, scrobbled_song_id, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                state.song_id,
                state.position_seconds.max(0.0),
                state.duration_seconds.max(0.0),
                state.was_playing as i64,
                state.app_volume.clamp(0.0, 2.0),
                state.now_playing_song_id,
                state.scrobbled_song_id,
                updated_at
            ],
        )?;
        self.get_playback_state()
    }

    fn with_queue_state(
        &self,
        mutate: impl FnOnce(&mut PlayQueue) -> CoreResult<()>,
    ) -> CoreResult<QueueState> {
        self.with_queue_result(|queue| {
            mutate(queue)?;
            Ok(())
        })
        .map(|(_, state)| state)
    }

    fn with_queue_result<T>(
        &self,
        mutate: impl FnOnce(&mut PlayQueue) -> CoreResult<T>,
    ) -> CoreResult<(T, QueueState)> {
        let mut queue = self.queue.lock().map_err(|_| CoreError::LockPoisoned)?;
        let result = mutate(&mut queue)?;
        let state = QueueState::from_queue(&mut queue);
        db::save_queue(&self.db_path, &state)?;
        Ok((result, state))
    }
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
    let conn = Connection::open(db_path)?;
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
    }
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

fn path_to_file_uri(path: &Path) -> String {
    url::Url::from_file_path(path)
        .map(|url| url.to_string())
        .unwrap_or_else(|_| format!("file://{}", path.to_string_lossy()))
}

fn should_prefetch_large_cover_art(size: Option<i32>) -> bool {
    size.is_some_and(|size| size > 0 && size < LARGE_COVER_ART_SIZE)
}

fn is_mobile_playback_cache_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(MOBILE_PLAYBACK_FORMAT))
}

#[cfg(test)]
mod tests {
    use super::{
        LARGE_COVER_ART_SIZE, MOBILE_PLAYBACK_FORMAT, StereodromeCore, path_to_file_uri,
        prune_stale_library_rows, should_prefetch_large_cover_art, write_sync_value,
    };
    use rusqlite::Connection;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn prefetches_large_cover_art_for_small_requests() {
        assert!(should_prefetch_large_cover_art(Some(128)));
        assert!(should_prefetch_large_cover_art(Some(
            LARGE_COVER_ART_SIZE - 1
        )));
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
        std::fs::write(cache_path, b"cached audio").expect("write cache file");

        let song_ids = core
            .get_offline_song_ids()
            .expect("offline song ids load");

        assert_eq!(song_ids, vec!["cached-song"]);
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

fn clamp_audio_processing_settings(settings: &mut AudioProcessingSettings) {
    if settings.normalization_mode != "album" {
        settings.normalization_mode = "track".to_string();
    }
    settings.target_lufs = settings.target_lufs.clamp(-24.0, -8.0);
    settings.preamp_db = settings.preamp_db.clamp(-12.0, 12.0);
    settings.crossfade_duration_ms = settings.crossfade_duration_ms.clamp(500, 15_000);
    if !matches!(
        settings.dynamics_preset.as_str(),
        "light" | "medium" | "heavy"
    ) {
        settings.dynamics_preset = "light".to_string();
    }
    if !matches!(
        settings.binaural_preset.as_str(),
        "light" | "medium" | "strong"
    ) {
        settings.binaural_preset = "medium".to_string();
    }
    settings.equalizer_bands_db.resize(12, 0.0);
    settings.equalizer_bands_db.truncate(12);
    for band in &mut settings.equalizer_bands_db {
        *band = band.clamp(-12.0, 12.0);
    }
}
