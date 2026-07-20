//! Message types for communication with the submarine client thread.

use serde::{Deserialize, Serialize};
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

/// Detailed artist info with albums
#[derive(Debug, Clone)]
pub struct ArtistDetail {
    pub album: Vec<AlbumInfo>,
}

/// Minimal artist entry used for full library reconciliation.
#[derive(Debug, Clone)]
pub struct ArtistSummaryInfo {
    pub id: String,
    pub name: String,
    pub album_count: i32,
    pub cover_art: Option<String>,
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

/// Minimal album entry used for newest-album incremental checks.
#[derive(Debug, Clone)]
pub struct AlbumSummaryInfo {
    pub id: String,
    pub artist_id: Option<String>,
    pub artist_name: Option<String>,
}

/// Album list type for getAlbumList2 queries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumListOrder {
    Newest,
    Recent,
    Frequent,
    Random,
    Highest,
    AlphabeticalByName,
    AlphabeticalByArtist,
    Starred,
}

impl AlbumListOrder {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "newest" => Some(Self::Newest),
            "recent" => Some(Self::Recent),
            "frequent" => Some(Self::Frequent),
            "random" => Some(Self::Random),
            "highest" => Some(Self::Highest),
            "alphabetical_by_name" => Some(Self::AlphabeticalByName),
            "alphabetical_by_artist" => Some(Self::AlphabeticalByArtist),
            "starred" => Some(Self::Starred),
            _ => None,
        }
    }

    pub fn to_submarine_order(self) -> submarine::api::get_album_list::Order {
        use submarine::api::get_album_list::Order;
        match self {
            Self::Newest => Order::Newest,
            Self::Recent => Order::Recent,
            Self::Frequent => Order::Frequent,
            Self::Random => Order::Random,
            Self::Highest => Order::Highest,
            Self::AlphabeticalByName => Order::AlphabeticalByName,
            Self::AlphabeticalByArtist => Order::AlphabeticalByArtist,
            Self::Starred => Order::Starred,
        }
    }
}

/// Album entry from getAlbumList2, richer than `AlbumSummaryInfo`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumListEntry {
    pub id: String,
    pub name: String,
    pub artist_id: Option<String>,
    #[serde(rename = "artistName")]
    pub artist_name: Option<String>,
    pub year: Option<i32>,
    pub song_count: Option<i32>,
    pub duration: Option<i32>,
    pub cover_art_id: Option<String>,
    pub play_count: Option<i64>,
    pub created: Option<String>,
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
            ClientError::ConnectionFailed(s) => write!(f, "Connection failed: {s}"),
            ClientError::ApiError(s) => write!(f, "API error: {s}"),
            ClientError::ChannelClosed => write!(f, "Client channel closed"),
            ClientError::Timeout => write!(f, "Request timed out"),
        }
    }
}

impl std::error::Error for ClientError {}

/// Playlist info from server
#[derive(Debug, Clone)]
pub struct PlaylistInfo {
    pub id: String,
    pub name: String,
    pub song_count: i32,
    pub duration: i32,
    pub owner: Option<String>,
    pub cover_art: Option<String>,
    pub created: String,
    pub changed: String,
}

/// Playlist with song entries from server
#[derive(Debug, Clone)]
pub struct PlaylistDetail {
    pub info: PlaylistInfo,
    pub entries: Vec<SongInfo>,
}

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
    /// Get all artists
    GetArtists {
        response_tx: oneshot::Sender<ClientResult<Vec<ArtistSummaryInfo>>>,
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
    /// Get newest albums page
    GetNewestAlbums {
        size: usize,
        offset: usize,
        response_tx: oneshot::Sender<ClientResult<Vec<AlbumSummaryInfo>>>,
    },
    /// Get album list by type (newest, recent, frequent, etc.)
    GetAlbumList {
        order: AlbumListOrder,
        size: usize,
        offset: usize,
        response_tx: oneshot::Sender<ClientResult<Vec<AlbumListEntry>>>,
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

    // === Playlists ===
    /// Get all playlists
    GetPlaylists {
        response_tx: oneshot::Sender<ClientResult<Vec<PlaylistInfo>>>,
    },
    /// Get playlist with songs
    GetPlaylist {
        playlist_id: String,
        response_tx: oneshot::Sender<ClientResult<PlaylistDetail>>,
    },
    /// Create a new playlist
    CreatePlaylist {
        name: String,
        song_ids: Vec<String>,
        response_tx: oneshot::Sender<ClientResult<PlaylistDetail>>,
    },
    /// Update a playlist (rename, add/remove songs)
    UpdatePlaylist {
        playlist_id: String,
        name: Option<String>,
        song_ids_to_add: Vec<String>,
        song_indexes_to_remove: Vec<i64>,
        response_tx: oneshot::Sender<ClientResult<()>>,
    },
    /// Delete a playlist
    DeletePlaylist {
        playlist_id: String,
        response_tx: oneshot::Sender<ClientResult<()>>,
    },

    // === Control ===
    /// Shutdown the client thread
    Shutdown,
}
