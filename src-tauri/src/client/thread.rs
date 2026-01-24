//! Client thread that processes submarine client requests.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use log::debug;
use submarine::{auth::AuthBuilder, Client};
use tokio::sync::mpsc;

use super::handle::SubsonicClientHandle;
use super::messages::*;

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

    /// Main event loop
    async fn run(&mut self) {
        debug!("Client thread started");
        loop {
            match self.request_rx.recv().await {
                Some(request) => {
                    if matches!(request, ClientRequest::Shutdown) {
                        debug!("Client thread shutting down");
                        break;
                    }
                    self.handle_request(request).await;
                }
                None => {
                    debug!("Client channel closed, shutting down");
                    break;
                }
            }
        }
    }

    /// Handle a single request
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
                let result = self.handle_get_artist(&artist_id).await;
                let _ = response_tx.send(result);
            }
            ClientRequest::GetAlbum {
                album_id,
                response_tx,
            } => {
                let result = self.handle_get_album(&album_id).await;
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

    fn handle_disconnect(&mut self) {
        self.client = None;
        self.server_config = None;
        self.connected.store(false, Ordering::SeqCst);
        debug!("Disconnected from server");
    }

    async fn handle_ping(&self) -> ClientResult<String> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        let ping = client
            .ping()
            .await
            .map_err(|e| ClientError::ApiError(e.to_string()))?;

        Ok(ping.version)
    }

    async fn handle_get_artists(&self) -> ClientResult<Vec<ArtistIndex>> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        let indexes = client
            .get_artists(None)
            .await
            .map_err(|e| ClientError::ApiError(e.to_string()))?;

        Ok(indexes
            .into_iter()
            .map(|index| ArtistIndex {
                artist: index
                    .artist
                    .into_iter()
                    .map(|a| ArtistInfo {
                        id: a.id,
                        name: a.name,
                        album_count: a.album_count,
                        cover_art: a.cover_art,
                    })
                    .collect(),
            })
            .collect())
    }

    async fn handle_get_artist(&self, artist_id: &str) -> ClientResult<ArtistDetail> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        let artist = client
            .get_artist(artist_id)
            .await
            .map_err(|e| ClientError::ApiError(e.to_string()))?;

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

    async fn handle_get_album(&self, album_id: &str) -> ClientResult<AlbumDetail> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        let album = client
            .get_album(album_id)
            .await
            .map_err(|e| ClientError::ApiError(e.to_string()))?;

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

    async fn handle_get_scan_status(&self) -> ClientResult<ScanStatusInfo> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        let status = client
            .get_scan_status()
            .await
            .map_err(|e| ClientError::ApiError(e.to_string()))?;

        Ok(ScanStatusInfo {
            scanning: status.scanning,
            count: status.count,
        })
    }

    async fn handle_start_scan(&self) -> ClientResult<ScanStatusInfo> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        let status = client
            .start_scan()
            .await
            .map_err(|e| ClientError::ApiError(e.to_string()))?;

        Ok(ScanStatusInfo {
            scanning: status.scanning,
            count: status.count,
        })
    }

    async fn handle_stream(&self, song_id: &str) -> ClientResult<Vec<u8>> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        client
            .stream(
                song_id,
                None::<i32>,    // max_bit_rate
                None::<String>, // format
                None::<i64>,    // time_offset
                None::<String>, // size
                None,           // estimate_content_length
                None,           // converted
            )
            .await
            .map_err(|e| ClientError::ApiError(e.to_string()))
    }

    async fn handle_get_cover_art(
        &self,
        cover_art_id: &str,
        size: Option<i32>,
    ) -> ClientResult<Vec<u8>> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        client
            .get_cover_art(cover_art_id, size)
            .await
            .map_err(|e| ClientError::ApiError(e.to_string()))
    }

    async fn handle_scrobble(
        &self,
        song_id: &str,
        time: Option<usize>,
        submission: Option<bool>,
    ) -> ClientResult<()> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        client
            .scrobble(vec![(song_id.to_string(), time)], submission)
            .await
            .map(|_| ())
            .map_err(|e| ClientError::ApiError(e.to_string()))
    }

    async fn handle_get_now_playing(&self) -> ClientResult<NowPlayingInfo> {
        let client = self.client.as_ref().ok_or(ClientError::NotConnected)?;

        let now_playing = client
            .get_now_playing()
            .await
            .map_err(|e| ClientError::ApiError(e.to_string()))?;

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
}

/// Spawn the client thread and return a handle
pub fn spawn() -> SubsonicClientHandle {
    ClientThread::spawn()
}
