//! Handle for communicating with the submarine client thread.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use super::messages::*;

/// Handle for communicating with the client thread.
/// This is stored in AppState and used by commands.
/// Clone-able and lock-free for reads.
#[derive(Clone)]
pub struct SubsonicClientHandle {
    request_tx: mpsc::Sender<ClientRequest>,
    connected: Arc<AtomicBool>,
}

impl SubsonicClientHandle {
    /// Create a new handle (called by thread::spawn)
    pub(super) fn new(request_tx: mpsc::Sender<ClientRequest>, connected: Arc<AtomicBool>) -> Self {
        Self {
            request_tx,
            connected,
        }
    }

    /// Check if connected (fast, no lock needed)
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    // === Authentication ===

    /// Connect to a Subsonic server
    pub async fn connect(
        &self,
        url: &str,
        username: &str,
        password: &str,
    ) -> ClientResult<ConnectionInfo> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::Connect {
                url: url.to_string(),
                username: username.to_string(),
                password: password.to_string(),
                response_tx,
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Disconnect from the server
    pub async fn disconnect(&self) -> ClientResult<()> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::Disconnect { response_tx })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Test connection with ping
    pub async fn ping(&self) -> ClientResult<String> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::Ping { response_tx })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    // === Library ===

    /// Get all artist indexes
    pub async fn get_artists(&self) -> ClientResult<Vec<ArtistIndex>> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::GetArtists { response_tx })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Get artist details with albums
    pub async fn get_artist(&self, artist_id: &str) -> ClientResult<ArtistDetail> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::GetArtist {
                artist_id: artist_id.to_string(),
                response_tx,
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Get album details with songs
    pub async fn get_album(&self, album_id: &str) -> ClientResult<AlbumDetail> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::GetAlbum {
                album_id: album_id.to_string(),
                response_tx,
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Get library scan status
    pub async fn get_scan_status(&self) -> ClientResult<ScanStatusInfo> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::GetScanStatus { response_tx })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Start library scan
    pub async fn start_scan(&self) -> ClientResult<ScanStatusInfo> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::StartScan { response_tx })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    // === Audio Streaming ===

    /// Stream audio for a song
    pub async fn stream(&self, song_id: &str) -> ClientResult<Vec<u8>> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::Stream {
                song_id: song_id.to_string(),
                response_tx,
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    // === Cover Art ===

    /// Get cover art image
    pub async fn get_cover_art(
        &self,
        cover_art_id: &str,
        size: Option<i32>,
    ) -> ClientResult<Vec<u8>> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::GetCoverArt {
                cover_art_id: cover_art_id.to_string(),
                size,
                response_tx,
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    // === Scrobbling ===

    /// Scrobble a song (now playing or submit)
    pub async fn scrobble(
        &self,
        song_id: &str,
        time: Option<usize>,
        submission: Option<bool>,
    ) -> ClientResult<()> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::Scrobble {
                song_id: song_id.to_string(),
                time,
                submission,
                response_tx,
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Get now playing list from server
    pub async fn get_now_playing(&self) -> ClientResult<NowPlayingInfo> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::GetNowPlaying { response_tx })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    // === Playlists ===

    /// Get all playlists from server
    pub async fn get_playlists(&self) -> ClientResult<Vec<PlaylistInfo>> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::GetPlaylists { response_tx })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Get playlist with songs from server
    pub async fn get_playlist(&self, playlist_id: &str) -> ClientResult<PlaylistDetail> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::GetPlaylist {
                playlist_id: playlist_id.to_string(),
                response_tx,
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Create a new playlist on server
    pub async fn create_playlist(
        &self,
        name: &str,
        song_ids: Vec<String>,
    ) -> ClientResult<PlaylistDetail> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::CreatePlaylist {
                name: name.to_string(),
                song_ids,
                response_tx,
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Update a playlist on server (rename, add/remove songs)
    pub async fn update_playlist(
        &self,
        playlist_id: &str,
        name: Option<String>,
        song_ids_to_add: Vec<String>,
        song_indexes_to_remove: Vec<i64>,
    ) -> ClientResult<()> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::UpdatePlaylist {
                playlist_id: playlist_id.to_string(),
                name,
                song_ids_to_add,
                song_indexes_to_remove,
                response_tx,
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    /// Delete a playlist from server
    pub async fn delete_playlist(&self, playlist_id: &str) -> ClientResult<()> {
        let (response_tx, response_rx) = oneshot::channel();

        self.request_tx
            .send(ClientRequest::DeletePlaylist {
                playlist_id: playlist_id.to_string(),
                response_tx,
            })
            .await
            .map_err(|_| ClientError::ChannelClosed)?;

        response_rx.await.map_err(|_| ClientError::ChannelClosed)?
    }

    // === Control ===

    /// Shutdown the client thread (called on app exit)
    pub fn shutdown(&self) {
        let _ = self.request_tx.try_send(ClientRequest::Shutdown);
    }
}
