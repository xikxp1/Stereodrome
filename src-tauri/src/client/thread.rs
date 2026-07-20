//! Client thread that processes submarine client requests.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use log::{debug, error, info, warn};
use submarine::{Client, api::get_album_list::Order, auth::AuthBuilder};
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::handle::SubsonicClientHandle;
use super::messages::{
    AlbumDetail, AlbumInfo, AlbumListEntry, AlbumListOrder, AlbumSummaryInfo, ArtistDetail,
    ArtistSummaryInfo, ClientError, ClientRequest, ClientResult, ConnectionInfo, NowPlayingChild,
    NowPlayingEntryInfo, NowPlayingInfo, PlaylistDetail, PlaylistInfo, ScanStatusInfo,
    ServerConfig, SongInfo,
};

/// Timeout for API requests (get artists, albums, etc.)
const API_TIMEOUT: Duration = Duration::from_secs(15);
/// Timeout for streaming audio
const STREAM_TIMEOUT: Duration = Duration::from_mins(1);
/// Timeout for ping/connection validation
const PING_TIMEOUT: Duration = Duration::from_secs(5);
/// Interval between connection validation pings
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Interval between background reconnect attempts after a transient disconnect
const RECONNECT_INTERVAL: Duration = Duration::from_secs(10);

/// Internal state of the client thread
struct ClientThread {
    /// The submarine client (None when disconnected)
    client: Option<Client>,
    /// Server config for reconnection
    server_config: Option<ServerConfig>,
    /// Request receiver
    request_rx: mpsc::Receiver<ClientRequest>,
    /// Connected flag (atomic for fast reads from outside)
    connected: Arc<AtomicBool>,
}

impl ClientThread {
    /// Spawn the client thread and return a handle for communication
    pub fn spawn() -> SubsonicClientHandle {
        let (request_tx, request_rx) = mpsc::channel::<ClientRequest>(100);
        let connected = Arc::new(AtomicBool::new(false));
        let connected_clone = Arc::clone(&connected);

        thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("Failed to create tokio runtime for client thread");

            rt.block_on(async {
                let mut thread = ClientThread {
                    client: None,
                    server_config: None,
                    request_rx,
                    connected: connected_clone,
                };
                thread.run().await;
            });
        });

        SubsonicClientHandle::new(request_tx, connected)
    }

    /// Main event loop with periodic connection validation
    async fn run(&mut self) {
        debug!("Client thread started");
        let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut reconnect = tokio::time::interval(RECONNECT_INTERVAL);
        reconnect.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                // Heartbeat: validate connection periodically
                _ = heartbeat.tick() => {
                    if self.client.is_some()
                        && let Err(e) = self.validate_connection().await {
                            warn!("Connection validation failed: {e}, retrying in background");
                            self.handle_connection_lost();
                            let _ = self.try_reconnect().await;
                        }
                }
                // Background reconnect: retry while we still have saved server config
                _ = reconnect.tick() => {
                    let _ = self.try_reconnect().await;
                }
                // Handle incoming requests
                request = self.request_rx.recv() => {
                    match request {
                        Some(ClientRequest::Shutdown) => {
                            debug!("Client thread shutdown requested");
                            break;
                        }
                        Some(req) => self.handle_request(req).await,
                        None => {
                            debug!("Client channel closed, shutting down");
                            break;
                        }
                    }
                }
            }
        }
        debug!("Client thread stopped");
    }

    /// Validate connection by pinging the server
    async fn validate_connection(&self) -> ClientResult<()> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;
        timeout(PING_TIMEOUT, client.ping())
            .await
            .map_err(|_| ClientError::Timeout)?
            .map_err(|e| ClientError::ApiError(e.to_string()))?;
        debug!("Connection validated");
        Ok(())
    }

    async fn try_reconnect(&mut self) -> ClientResult<()> {
        if self.client.is_some() {
            return Ok(());
        }

        let Some(config) = self.server_config.clone() else {
            return Ok(());
        };

        info!("Attempting background reconnect to {}", config.url);
        match self
            .handle_connect(&config.url, &config.username, &config.password)
            .await
        {
            Ok(result) => {
                info!(
                    "Background reconnect succeeded, server version {}",
                    result.server_version
                );
                Ok(())
            }
            Err(e) => {
                warn!("Background reconnect failed: {e}");
                Err(e)
            }
        }
    }

    /// Handle a single request
    #[allow(clippy::too_many_lines)]
    async fn handle_request(&mut self, request: ClientRequest) {
        match request {
            // === Authentication ===
            ClientRequest::Connect {
                url,
                username,
                password,
                response_tx,
            } => {
                let result = self.handle_connect(&url, &username, &password).await;
                let _ = response_tx.send(result);
            }
            ClientRequest::Disconnect { response_tx } => {
                self.handle_disconnect();
                let _ = response_tx.send(Ok(()));
            }
            ClientRequest::Ping { response_tx } => {
                let result = self.handle_ping().await;
                let _ = response_tx.send(result);
            }

            // === Library ===
            ClientRequest::GetArtists { response_tx } => {
                let result = self.handle_get_artists().await;
                let _ = response_tx.send(result);
            }
            ClientRequest::GetArtist {
                artist_id,
                response_tx,
            } => {
                let result = self.client.clone().ok_or(ClientError::NotConnected);
                match result {
                    Ok(client) => {
                        tokio::spawn(async move {
                            let result = Self::fetch_artist(&client, &artist_id).await;
                            let _ = response_tx.send(result);
                        });
                    }
                    Err(e) => {
                        let _ = response_tx.send(Err(e));
                    }
                }
            }
            ClientRequest::GetAlbum {
                album_id,
                response_tx,
            } => {
                let result = self.client.clone().ok_or(ClientError::NotConnected);
                match result {
                    Ok(client) => {
                        tokio::spawn(async move {
                            let result = Self::fetch_album(&client, &album_id).await;
                            let _ = response_tx.send(result);
                        });
                    }
                    Err(e) => {
                        let _ = response_tx.send(Err(e));
                    }
                }
            }
            ClientRequest::GetNewestAlbums {
                size,
                offset,
                response_tx,
            } => {
                let result = self.handle_get_newest_albums(size, offset).await;
                let _ = response_tx.send(result);
            }
            ClientRequest::GetAlbumList {
                order,
                size,
                offset,
                response_tx,
            } => {
                let result = self.handle_get_album_list(order, size, offset).await;
                let _ = response_tx.send(result);
            }
            ClientRequest::GetScanStatus { response_tx } => {
                let result = self.handle_get_scan_status().await;
                let _ = response_tx.send(result);
            }
            ClientRequest::StartScan { response_tx } => {
                let result = self.handle_start_scan().await;
                let _ = response_tx.send(result);
            }

            // === Audio Streaming ===
            ClientRequest::Stream {
                song_id,
                response_tx,
            } => {
                let result = self.handle_stream(&song_id).await;
                let _ = response_tx.send(result);
            }

            // === Cover Art ===
            ClientRequest::GetCoverArt {
                cover_art_id,
                size,
                response_tx,
            } => {
                let result = self.handle_get_cover_art(&cover_art_id, size).await;
                let _ = response_tx.send(result);
            }

            // === Scrobbling ===
            ClientRequest::Scrobble {
                song_id,
                time,
                submission,
                response_tx,
            } => {
                let result = self.handle_scrobble(&song_id, time, submission).await;
                let _ = response_tx.send(result);
            }
            ClientRequest::GetNowPlaying { response_tx } => {
                let result = self.handle_get_now_playing().await;
                let _ = response_tx.send(result);
            }

            // === Playlists ===
            ClientRequest::GetPlaylists { response_tx } => {
                let result = self.handle_get_playlists().await;
                let _ = response_tx.send(result);
            }
            ClientRequest::GetPlaylist {
                playlist_id,
                response_tx,
            } => {
                let result = self.handle_get_playlist(&playlist_id).await;
                let _ = response_tx.send(result);
            }
            ClientRequest::CreatePlaylist {
                name,
                song_ids,
                response_tx,
            } => {
                let result = self.handle_create_playlist(&name, song_ids).await;
                let _ = response_tx.send(result);
            }
            ClientRequest::UpdatePlaylist {
                playlist_id,
                name,
                song_ids_to_add,
                song_indexes_to_remove,
                response_tx,
            } => {
                let result = self
                    .handle_update_playlist(
                        &playlist_id,
                        name,
                        song_ids_to_add,
                        song_indexes_to_remove,
                    )
                    .await;
                let _ = response_tx.send(result);
            }
            ClientRequest::DeletePlaylist {
                playlist_id,
                response_tx,
            } => {
                let result = self.handle_delete_playlist(&playlist_id).await;
                let _ = response_tx.send(result);
            }

            // Handled in run loop
            ClientRequest::Shutdown => unreachable!(),
        }
    }

    // === Handler implementations ===

    async fn handle_connect(
        &mut self,
        url: &str,
        username: &str,
        password: &str,
    ) -> ClientResult<ConnectionInfo> {
        let auth = AuthBuilder::new(username, "1.16.1")
            .client_name("Stereodrome")
            .hashed(password);

        let client = Client::new(url, auth);

        // Test connection
        let ping = client
            .ping()
            .await
            .map_err(|e| ClientError::ConnectionFailed(e.to_string()))?;

        self.server_config = Some(ServerConfig {
            url: url.to_string(),
            username: username.to_string(),
            password: password.to_string(),
        });
        self.client = Some(client);
        self.connected.store(true, Ordering::SeqCst);

        debug!("Connected to server version {}", ping.version);

        Ok(ConnectionInfo {
            server_version: ping.version,
        })
    }

    fn handle_connection_lost(&mut self) {
        self.client = None;
        self.connected.store(false, Ordering::SeqCst);
        debug!("Connection dropped; keeping server config for background reconnect");
    }

    fn handle_disconnect(&mut self) {
        self.client = None;
        self.server_config = None;
        self.connected.store(false, Ordering::SeqCst);
        debug!("Disconnected from server");
    }

    async fn handle_ping(&self) -> ClientResult<String> {
        debug!("Pinging server");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(PING_TIMEOUT, client.ping()).await {
            Ok(Ok(ping)) => {
                debug!("Ping successful: server version {}", ping.version);
                Ok(ping.version)
            }
            Ok(Err(e)) => {
                error!("Ping API error: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Ping timeout after {PING_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }

    async fn handle_get_artists(&self) -> ClientResult<Vec<ArtistSummaryInfo>> {
        debug!("Getting artists");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        let indexes = match timeout(API_TIMEOUT, client.get_artists(None::<String>)).await {
            Ok(Ok(indexes)) => indexes,
            Ok(Err(e)) => {
                error!("Get artists API error: {e}");
                return Err(ClientError::ApiError(e.to_string()));
            }
            Err(_) => {
                error!("Get artists timeout after {API_TIMEOUT:?}");
                return Err(ClientError::Timeout);
            }
        };

        let artists = indexes
            .into_iter()
            .flat_map(|index| index.artist)
            .map(|artist| ArtistSummaryInfo {
                id: artist.id,
                name: artist.name,
                album_count: artist.album_count,
                cover_art: artist.cover_art,
            })
            .collect::<Vec<_>>();

        debug!("Got {} artists", artists.len());
        Ok(artists)
    }

    async fn fetch_artist(client: &Client, artist_id: &str) -> ClientResult<ArtistDetail> {
        debug!("Getting artist: {artist_id}");

        let artist = match timeout(API_TIMEOUT, client.get_artist(artist_id)).await {
            Ok(Ok(artist)) => artist,
            Ok(Err(e)) => {
                error!("Get artist {artist_id} API error: {e}");
                return Err(ClientError::ApiError(e.to_string()));
            }
            Err(_) => {
                error!("Get artist {artist_id} timeout after {API_TIMEOUT:?}");
                return Err(ClientError::Timeout);
            }
        };

        debug!("Got artist with {} albums", artist.album.len());
        Ok(ArtistDetail {
            album: artist
                .album
                .into_iter()
                .map(|a| AlbumInfo {
                    id: a.id,
                    name: a.name,
                    year: a.year,
                    song_count: a.song_count,
                    duration: a.duration,
                    cover_art: a.cover_art,
                })
                .collect(),
        })
    }

    async fn fetch_album(client: &Client, album_id: &str) -> ClientResult<AlbumDetail> {
        debug!("Getting album: {album_id}");

        let album = match timeout(API_TIMEOUT, client.get_album(album_id)).await {
            Ok(Ok(album)) => album,
            Ok(Err(e)) => {
                error!("Get album {album_id} API error: {e}");
                return Err(ClientError::ApiError(e.to_string()));
            }
            Err(_) => {
                error!("Get album {album_id} timeout after {API_TIMEOUT:?}");
                return Err(ClientError::Timeout);
            }
        };

        debug!("Got album with {} songs", album.song.len());
        Ok(AlbumDetail {
            song: album
                .song
                .into_iter()
                .map(|s| SongInfo {
                    id: s.id,
                    title: s.title,
                    track: s.track,
                    disc_number: s.disc_number,
                    duration: s.duration,
                    bit_rate: s.bit_rate,
                    size: s.size,
                    suffix: s.suffix,
                    content_type: s.content_type,
                    path: s.path,
                    year: s.year,
                    genre: s.genre,
                })
                .collect(),
        })
    }

    async fn handle_get_newest_albums(
        &self,
        size: usize,
        offset: usize,
    ) -> ClientResult<Vec<AlbumSummaryInfo>> {
        debug!("Getting newest albums: size={size}, offset={offset}");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        let albums = match timeout(
            API_TIMEOUT,
            client.get_album_list2(Order::Newest, Some(size), Some(offset), None::<String>),
        )
        .await
        {
            Ok(Ok(albums)) => albums,
            Ok(Err(e)) => {
                error!("Get newest albums API error: {e}");
                return Err(ClientError::ApiError(e.to_string()));
            }
            Err(_) => {
                error!("Get newest albums timeout after {API_TIMEOUT:?}");
                return Err(ClientError::Timeout);
            }
        };

        Ok(albums
            .into_iter()
            .map(|album| AlbumSummaryInfo {
                id: album.id,
                artist_id: album.artist_id,
                artist_name: album.artist,
            })
            .collect())
    }

    async fn handle_get_album_list(
        &self,
        order: AlbumListOrder,
        size: usize,
        offset: usize,
    ) -> ClientResult<Vec<AlbumListEntry>> {
        debug!("Getting album list: order={order:?}, size={size}, offset={offset}");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;
        let sub_order = order.to_submarine_order();

        let albums = match timeout(
            API_TIMEOUT,
            client.get_album_list2(sub_order, Some(size), Some(offset), None::<String>),
        )
        .await
        {
            Ok(Ok(albums)) => albums,
            Ok(Err(e)) => {
                error!("Get album list ({order:?}) API error: {e}");
                return Err(ClientError::ApiError(e.to_string()));
            }
            Err(_) => {
                error!("Get album list ({order:?}) timeout after {API_TIMEOUT:?}");
                return Err(ClientError::Timeout);
            }
        };

        Ok(albums
            .into_iter()
            .map(|album| AlbumListEntry {
                id: album.id,
                name: if album.title.is_empty() {
                    album.name.clone()
                } else {
                    album.title.clone()
                },
                artist_id: album.artist_id,
                artist_name: album.artist,
                year: album.year,
                song_count: None, // submarine Child does not expose song_count
                duration: album.duration,
                cover_art_id: album.cover_art,
                play_count: album.play_count,
                created: album.created.map(|dt| dt.to_rfc3339()),
            })
            .collect())
    }

    async fn handle_get_scan_status(&self) -> ClientResult<ScanStatusInfo> {
        debug!("Getting scan status");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(API_TIMEOUT, client.get_scan_status()).await {
            Ok(Ok(status)) => {
                debug!(
                    "Scan status: scanning={}, count={:?}",
                    status.scanning, status.count
                );
                Ok(ScanStatusInfo {
                    scanning: status.scanning,
                    count: status.count,
                })
            }
            Ok(Err(e)) => {
                error!("Get scan status API error: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Get scan status timeout after {API_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }

    async fn handle_start_scan(&self) -> ClientResult<ScanStatusInfo> {
        debug!("Starting library scan");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(API_TIMEOUT, client.start_scan()).await {
            Ok(Ok(status)) => {
                debug!(
                    "Scan started: scanning={}, count={:?}",
                    status.scanning, status.count
                );
                Ok(ScanStatusInfo {
                    scanning: status.scanning,
                    count: status.count,
                })
            }
            Ok(Err(e)) => {
                error!("Start scan API error: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Start scan timeout after {API_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }

    async fn handle_stream(&self, song_id: &str) -> ClientResult<Vec<u8>> {
        debug!("Streaming song: {song_id}");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(
            STREAM_TIMEOUT,
            client.stream(
                song_id,
                None::<i32>,    // max_bit_rate
                None::<String>, // format
                None::<i64>,    // time_offset
                None::<String>, // size
                None,           // estimate_content_length
                None,           // converted
            ),
        )
        .await
        {
            Ok(Ok(data)) => {
                debug!("Stream complete for {}: {} bytes", song_id, data.len());
                Ok(data)
            }
            Ok(Err(e)) => {
                error!("Stream API error for {song_id}: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Stream timeout for {song_id} after {STREAM_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }

    async fn handle_get_cover_art(
        &self,
        cover_art_id: &str,
        size: Option<i32>,
    ) -> ClientResult<Vec<u8>> {
        debug!("Getting cover art: {cover_art_id}");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(API_TIMEOUT, client.get_cover_art(cover_art_id, size)).await {
            Ok(Ok(data)) => {
                debug!("Got cover art {}: {} bytes", cover_art_id, data.len());
                Ok(data)
            }
            Ok(Err(e)) => {
                error!("Get cover art {cover_art_id} API error: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Get cover art {cover_art_id} timeout after {API_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }

    async fn handle_scrobble(
        &self,
        song_id: &str,
        time: Option<usize>,
        submission: Option<bool>,
    ) -> ClientResult<()> {
        debug!("Scrobbling song: {song_id} (submission={submission:?})");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(
            API_TIMEOUT,
            client.scrobble(vec![(song_id.to_string(), time)], submission),
        )
        .await
        {
            Ok(Ok(_)) => {
                debug!("Scrobble successful for {song_id}");
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Scrobble API error for {song_id}: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Scrobble timeout for {song_id} after {API_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }

    async fn handle_get_now_playing(&self) -> ClientResult<NowPlayingInfo> {
        debug!("Getting now playing");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        let now_playing = match timeout(API_TIMEOUT, client.get_now_playing()).await {
            Ok(Ok(np)) => np,
            Ok(Err(e)) => {
                error!("Get now playing API error: {e}");
                return Err(ClientError::ApiError(e.to_string()));
            }
            Err(_) => {
                error!("Get now playing timeout after {API_TIMEOUT:?}");
                return Err(ClientError::Timeout);
            }
        };

        debug!("Got {} now playing entries", now_playing.entry.len());
        Ok(NowPlayingInfo {
            entry: now_playing
                .entry
                .into_iter()
                .map(|e| NowPlayingEntryInfo {
                    child: NowPlayingChild {
                        id: e.child.id,
                        title: e.child.title,
                        artist: e.child.artist,
                        album: e.child.album,
                        duration: e.child.duration,
                        cover_art: e.child.cover_art,
                    },
                    username: e.username,
                    minutes_ago: e.minutes_ago,
                    player_name: e.player_name,
                })
                .collect(),
        })
    }

    // === Playlist handlers ===

    async fn handle_get_playlists(&self) -> ClientResult<Vec<PlaylistInfo>> {
        debug!("Getting playlists");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(API_TIMEOUT, client.get_playlists(None::<String>)).await {
            Ok(Ok(playlists)) => {
                debug!("Got {} playlists", playlists.len());
                Ok(playlists
                    .into_iter()
                    .map(|p| PlaylistInfo {
                        id: p.id,
                        name: p.name,
                        song_count: p.song_count,
                        duration: p.duration,
                        owner: p.owner,
                        cover_art: p.cover_art,
                        created: p.created.to_rfc3339(),
                        changed: p.changed.to_rfc3339(),
                    })
                    .collect())
            }
            Ok(Err(e)) => {
                error!("Get playlists API error: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Get playlists timeout after {API_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }

    async fn handle_get_playlist(&self, playlist_id: &str) -> ClientResult<PlaylistDetail> {
        debug!("Getting playlist: {playlist_id}");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(API_TIMEOUT, client.get_playlist(playlist_id)).await {
            Ok(Ok(playlist)) => {
                debug!(
                    "Got playlist '{}' with {} entries",
                    playlist.base.name,
                    playlist.entry.len()
                );
                let info = PlaylistInfo {
                    id: playlist.base.id,
                    name: playlist.base.name,
                    song_count: playlist.base.song_count,
                    duration: playlist.base.duration,
                    owner: playlist.base.owner,
                    cover_art: playlist.base.cover_art,
                    created: playlist.base.created.to_rfc3339(),
                    changed: playlist.base.changed.to_rfc3339(),
                };
                let entries = playlist
                    .entry
                    .into_iter()
                    .map(|c| SongInfo {
                        id: c.id,
                        title: c.title,
                        track: c.track,
                        disc_number: c.disc_number,
                        duration: c.duration,
                        bit_rate: c.bit_rate,
                        size: c.size,
                        suffix: c.suffix,
                        content_type: c.content_type,
                        path: c.path,
                        year: c.year,
                        genre: c.genre,
                    })
                    .collect();
                Ok(PlaylistDetail { info, entries })
            }
            Ok(Err(e)) => {
                error!("Get playlist {playlist_id} API error: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Get playlist {playlist_id} timeout after {API_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }

    async fn handle_create_playlist(
        &self,
        name: &str,
        song_ids: Vec<String>,
    ) -> ClientResult<PlaylistDetail> {
        debug!("Creating playlist: {name}");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(API_TIMEOUT, client.create_playlist(name, song_ids)).await {
            Ok(Ok(playlist)) => {
                debug!("Created playlist: {}", playlist.base.id);
                let info = PlaylistInfo {
                    id: playlist.base.id,
                    name: playlist.base.name,
                    song_count: playlist.base.song_count,
                    duration: playlist.base.duration,
                    owner: playlist.base.owner,
                    cover_art: playlist.base.cover_art,
                    created: playlist.base.created.to_rfc3339(),
                    changed: playlist.base.changed.to_rfc3339(),
                };
                let entries = playlist
                    .entry
                    .into_iter()
                    .map(|c| SongInfo {
                        id: c.id,
                        title: c.title,
                        track: c.track,
                        disc_number: c.disc_number,
                        duration: c.duration,
                        bit_rate: c.bit_rate,
                        size: c.size,
                        suffix: c.suffix,
                        content_type: c.content_type,
                        path: c.path,
                        year: c.year,
                        genre: c.genre,
                    })
                    .collect();
                Ok(PlaylistDetail { info, entries })
            }
            Ok(Err(e)) => {
                error!("Create playlist API error: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Create playlist timeout after {API_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }

    async fn handle_update_playlist(
        &self,
        playlist_id: &str,
        name: Option<String>,
        song_ids_to_add: Vec<String>,
        song_indexes_to_remove: Vec<i64>,
    ) -> ClientResult<()> {
        debug!("Updating playlist: {playlist_id}");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(
            API_TIMEOUT,
            client.update_playlist(
                playlist_id,
                name,
                None::<String>,
                None,
                song_ids_to_add,
                song_indexes_to_remove,
            ),
        )
        .await
        {
            Ok(Ok(_)) => {
                debug!("Updated playlist: {playlist_id}");
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Update playlist {playlist_id} API error: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Update playlist {playlist_id} timeout after {API_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }

    async fn handle_delete_playlist(&self, playlist_id: &str) -> ClientResult<()> {
        debug!("Deleting playlist: {playlist_id}");
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        match timeout(API_TIMEOUT, client.delete_playlist(playlist_id)).await {
            Ok(Ok(_)) => {
                debug!("Deleted playlist: {playlist_id}");
                Ok(())
            }
            Ok(Err(e)) => {
                error!("Delete playlist {playlist_id} API error: {e}");
                Err(ClientError::ApiError(e.to_string()))
            }
            Err(_) => {
                error!("Delete playlist {playlist_id} timeout after {API_TIMEOUT:?}");
                Err(ClientError::Timeout)
            }
        }
    }
}

/// Spawn the client thread and return a handle
pub fn spawn() -> SubsonicClientHandle {
    ClientThread::spawn()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_thread() -> ClientThread {
        let (_request_tx, request_rx) = mpsc::channel(1);

        ClientThread {
            client: None,
            server_config: None,
            request_rx,
            connected: Arc::new(AtomicBool::new(true)),
        }
    }

    #[test]
    fn transient_disconnect_preserves_server_config() {
        let mut thread = make_thread();
        thread.server_config = Some(ServerConfig {
            url: "https://example.com".to_string(),
            username: "user".to_string(),
            password: "pass".to_string(),
        });

        thread.handle_connection_lost();

        assert!(thread.server_config.is_some());
        assert!(!thread.connected.load(Ordering::SeqCst));
    }

    #[test]
    fn explicit_disconnect_clears_server_config() {
        let mut thread = make_thread();
        thread.server_config = Some(ServerConfig {
            url: "https://example.com".to_string(),
            username: "user".to_string(),
            password: "pass".to_string(),
        });

        thread.handle_disconnect();

        assert!(thread.server_config.is_none());
        assert!(!thread.connected.load(Ordering::SeqCst));
    }
}
