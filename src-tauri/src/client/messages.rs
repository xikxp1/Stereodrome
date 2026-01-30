//! Message types for communication with the submarine client thread.

use tokio::sync::oneshot;

/// Server configuration for connection
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub url: String,
    pub username: String,
    pub password: String,
}

/// Connection info returned after successful connect
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub server_version: String,
}

/// Artist index from getArtists API
#[derive(Debug, Clone)]
pub struct ArtistIndex {
    pub artist: Vec<ArtistInfo>,
}

/// Basic artist info from index
#[derive(Debug, Clone)]
pub struct ArtistInfo {
    pub id: String,
    pub name: String,
    pub album_count: i32,
    pub cover_art: Option<String>,
}

/// Detailed artist info with albums
#[derive(Debug, Clone)]
pub struct ArtistDetail {
    pub album: Vec<AlbumInfo>,
}

/// Album info
#[derive(Debug, Clone)]
pub struct AlbumInfo {
    pub id: String,
    pub name: String,
    pub year: Option<i32>,
    pub song_count: i32,
    pub duration: i32,
    pub cover_art: Option<String>,
}

/// Detailed album info with songs
#[derive(Debug, Clone)]
pub struct AlbumDetail {
    pub song: Vec<SongInfo>,
}

/// Song info
#[derive(Debug, Clone)]
pub struct SongInfo {
    pub id: String,
    pub title: String,
    pub track: Option<i32>,
    pub disc_number: Option<i32>,
    pub duration: Option<i32>,
    pub bit_rate: Option<i32>,
    pub size: Option<i64>,
    pub suffix: Option<String>,
    pub content_type: Option<String>,
    pub path: Option<String>,
    pub year: Option<i32>,
    pub genre: Option<String>,
}

/// Scan status from server
#[derive(Debug, Clone)]
pub struct ScanStatusInfo {
    pub scanning: bool,
    pub count: Option<i64>,
}

/// Now playing entry
#[derive(Debug, Clone)]
pub struct NowPlayingInfo {
    pub entry: Vec<NowPlayingEntryInfo>,
}

/// Single now playing entry
#[derive(Debug, Clone)]
pub struct NowPlayingEntryInfo {
    pub child: NowPlayingChild,
    pub username: String,
    pub minutes_ago: i32,
    pub player_name: Option<String>,
}

/// Child element in now playing entry
#[derive(Debug, Clone)]
pub struct NowPlayingChild {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<i32>,
    pub cover_art: Option<String>,
}

/// Result type for client operations
pub type ClientResult<T> = Result<T, ClientError>;

/// Errors from client operations
#[derive(Debug, Clone)]
pub enum ClientError {
    /// Not connected to server
    NotConnected,
    /// Connection attempt failed
    ConnectionFailed(String),
    /// API call failed
    ApiError(String),
    /// Channel communication failed
    ChannelClosed,
    /// Request timed out
    Timeout,
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::NotConnected => write!(f, "Not connected to server"),
            ClientError::ConnectionFailed(s) => write!(f, "Connection failed: {}", s),
            ClientError::ApiError(s) => write!(f, "API error: {}", s),
            ClientError::ChannelClosed => write!(f, "Client channel closed"),
            ClientError::Timeout => write!(f, "Request timed out"),
        }
    }
}

impl std::error::Error for ClientError {}

/// All possible requests to the Subsonic client thread
pub enum ClientRequest {
    // === Authentication ===
    /// Connect to a Subsonic server
    Connect {
        url: String,
        username: String,
        password: String,
        response_tx: oneshot::Sender<ClientResult<ConnectionInfo>>,
    },
    /// Disconnect from the server
    Disconnect {
        response_tx: oneshot::Sender<ClientResult<()>>,
    },
    /// Test connection with ping
    Ping {
        response_tx: oneshot::Sender<ClientResult<String>>,
    },

    // === Library ===
    /// Get all artist indexes
    GetArtists {
        response_tx: oneshot::Sender<ClientResult<Vec<ArtistIndex>>>,
    },
    /// Get artist details with albums
    GetArtist {
        artist_id: String,
        response_tx: oneshot::Sender<ClientResult<ArtistDetail>>,
    },
    /// Get album details with songs
    GetAlbum {
        album_id: String,
        response_tx: oneshot::Sender<ClientResult<AlbumDetail>>,
    },
    /// Get library scan status
    GetScanStatus {
        response_tx: oneshot::Sender<ClientResult<ScanStatusInfo>>,
    },
    /// Start library scan
    StartScan {
        response_tx: oneshot::Sender<ClientResult<ScanStatusInfo>>,
    },

    // === Audio Streaming ===
    /// Stream audio for a song
    Stream {
        song_id: String,
        response_tx: oneshot::Sender<ClientResult<Vec<u8>>>,
    },

    // === Cover Art ===
    /// Get cover art image
    GetCoverArt {
        cover_art_id: String,
        size: Option<i32>,
        response_tx: oneshot::Sender<ClientResult<Vec<u8>>>,
    },

    // === Now Playing / Scrobbling ===
    /// Scrobble a song (now playing or submit)
    Scrobble {
        song_id: String,
        time: Option<usize>,
        submission: Option<bool>,
        response_tx: oneshot::Sender<ClientResult<()>>,
    },
    /// Get now playing list from server
    GetNowPlaying {
        response_tx: oneshot::Sender<ClientResult<NowPlayingInfo>>,
    },

    // === Control ===
    /// Shutdown the client thread
    Shutdown,
}
