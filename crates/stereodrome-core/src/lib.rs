mod db;
mod error;
mod models;
pub mod queue;
mod subsonic;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Utc;
use queue::{PlayQueue, QueueItem, QueueState, RepeatMode};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use submarine::{Client, api::get_album_list::Order, auth::AuthBuilder};
use tokio::sync::Mutex as AsyncMutex;

pub use error::{CoreError, CoreResult};
pub use models::*;
pub use queue::{QueueItem as SharedQueueItem, QueueState as SharedQueueState};

const API_VERSION: &str = "1.16.1";
const CLIENT_NAME: &str = "StereodromeMobile";

#[derive(Debug)]
pub struct StereodromeCore {
    db_path: PathBuf,
    config_path: PathBuf,
    server_config: Mutex<Option<ServerConfig>>,
    client: AsyncMutex<Option<Client>>,
    queue: Mutex<PlayQueue>,
}

impl StereodromeCore {
    pub fn new(data_dir: impl AsRef<Path>) -> CoreResult<Self> {
        let data_dir = data_dir.as_ref();
        std::fs::create_dir_all(data_dir)?;
        std::fs::create_dir_all(data_dir.join("cover_art"))?;
        let db_path = data_dir.join("stereodrome.db");
        let config_path = data_dir.join("server_config.json");
        db::init(&db_path)?;
        let server_config = read_server_config(&config_path)?;
        let queue = db::load_queue(&db_path)?;

        Ok(Self {
            db_path,
            config_path,
            server_config: Mutex::new(server_config),
            client: AsyncMutex::new(None),
            queue: Mutex::new(queue),
        })
    }

    pub async fn connect_server(&self, params: ConnectParams) -> CoreResult<ConnectionStatus> {
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

        Ok(ConnectionStatus {
            connected: true,
            server_url: Some(config.url),
            username: Some(config.username),
            server_version: Some(ping.version),
        })
    }

    pub async fn restore_session(&self) -> CoreResult<ConnectionStatus> {
        let config = {
            self.server_config
                .lock()
                .map_err(|_| CoreError::LockPoisoned)?
                .clone()
        };

        let Some(config) = config else {
            return Ok(ConnectionStatus::disconnected());
        };

        let client = build_client(&config.url, &config.username, &config.password);
        match client.ping().await {
            Ok(ping) => {
                *self.client.lock().await = Some(client);
                Ok(ConnectionStatus {
                    connected: true,
                    server_url: Some(config.url),
                    username: Some(config.username),
                    server_version: Some(ping.version),
                })
            }
            Err(_) => Ok(ConnectionStatus {
                connected: false,
                server_url: Some(config.url),
                username: Some(config.username),
                server_version: None,
            }),
        }
    }

    pub async fn disconnect_server(&self) -> CoreResult<()> {
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
        let client = self.connected_client().await?;
        let indexes = client
            .get_artists(None::<String>)
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        let artists = indexes
            .into_iter()
            .flat_map(|index| index.artist)
            .collect::<Vec<_>>();

        let mut result = SyncResult::default();
        let now = Utc::now().to_rfc3339();
        let mut conn = Connection::open(&self.db_path)?;
        let tx = conn.transaction()?;

        for artist in artists {
            tx.execute(
                "INSERT OR REPLACE INTO artists
                 (id, name, album_count, cover_art_id, synced_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    artist.id,
                    artist.name,
                    artist.album_count,
                    artist.cover_art,
                    now
                ],
            )?;
            result.artists += 1;

            let artist_detail = client
                .get_artist(&artist.id)
                .await
                .map_err(|e| CoreError::Subsonic(e.to_string()))?;
            for album in artist_detail.album {
                tx.execute(
                    "INSERT OR REPLACE INTO albums
                     (id, artist_id, name, year, song_count, duration, cover_art_id, synced_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        album.id,
                        artist.id,
                        album.name,
                        album.year,
                        album.song_count,
                        album.duration,
                        album.cover_art,
                        now
                    ],
                )?;
                result.albums += 1;

                let album_detail = client
                    .get_album(&album.id)
                    .await
                    .map_err(|e| CoreError::Subsonic(e.to_string()))?;
                for song in album_detail.song {
                    tx.execute(
                        "INSERT OR REPLACE INTO songs
                         (id, album_id, artist_id, title, track_number, disc_number, duration,
                          bit_rate, size, suffix, content_type, path, year, genre, synced_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                        params![
                            song.id,
                            album.id,
                            artist.id,
                            song.title,
                            song.track,
                            song.disc_number.unwrap_or(1),
                            song.duration,
                            song.bit_rate,
                            song.size,
                            song.suffix,
                            song.content_type,
                            song.path,
                            song.year,
                            song.genre,
                            now
                        ],
                    )?;
                    result.songs += 1;
                }
            }
        }

        tx.execute(
            "INSERT OR REPLACE INTO sync_state (key, value, updated_at)
             VALUES ('library_last_success_at', ?1, ?1)",
            [&now],
        )?;
        tx.commit()?;
        Ok(result)
    }

    pub fn get_library_sync_status(&self) -> CoreResult<LibrarySyncStatus> {
        let conn = Connection::open(&self.db_path)?;
        let last_success_at = conn
            .query_row(
                "SELECT value FROM sync_state WHERE key = 'library_last_success_at'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;

        Ok(LibrarySyncStatus {
            active_job: None,
            incremental: SyncJobStatus {
                enabled: false,
                interval_minutes: 0,
                running: false,
                last_attempt_at: last_success_at.clone(),
                last_success_at: last_success_at.clone(),
                last_error: None,
                next_run_at: None,
            },
            full_reconcile: SyncJobStatus {
                enabled: false,
                interval_minutes: 0,
                running: false,
                last_attempt_at: last_success_at.clone(),
                last_success_at,
                last_error: None,
                next_run_at: None,
            },
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
        let client = self.connected_client().await?;
        let playlists = client
            .get_playlists(None::<String>)
            .await
            .map_err(|e| CoreError::Subsonic(e.to_string()))?;
        Ok(playlists
            .into_iter()
            .map(|playlist| Playlist {
                id: playlist.id,
                name: playlist.name,
                song_count: playlist.song_count,
                duration: playlist.duration,
                owner: playlist.owner,
                cover_art_id: playlist.cover_art,
                created_at: playlist.created.to_rfc3339(),
                changed_at: playlist.changed.to_rfc3339(),
            })
            .collect())
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

    pub fn get_stream_uri(&self, song_id: String) -> CoreResult<String> {
        let config = self.current_config()?;
        Ok(subsonic::signed_url(
            &config,
            "stream",
            &[("id", song_id.as_str())],
        ))
    }

    pub fn get_cover_art_uri(&self, cover_art_id: String, size: Option<i32>) -> CoreResult<String> {
        let config = self.current_config()?;
        let size_string = size.map(|s| s.to_string());
        let mut params = vec![("id", cover_art_id.as_str())];
        if let Some(size) = size_string.as_deref() {
            params.push(("size", size));
        }
        Ok(subsonic::signed_url(&config, "getCoverArt", &params))
    }

    pub fn get_song_cover_art_uri(
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

        cover_art_id
            .map(|id| self.get_cover_art_uri(id, size))
            .transpose()
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
