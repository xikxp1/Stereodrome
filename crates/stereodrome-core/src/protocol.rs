//! Stable, versioned messages exchanged with a [`StereodromeRuntimeHandle`](crate::StereodromeRuntimeHandle).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::queue::{QueueItem, RepeatMode};
use crate::{
    AudioProcessingSettings, ConnectParams, ConnectivitySettings, CoreError, LibrarySyncStatus,
    PlaybackProgress, PlaybackState, QueueState, ServerSettingsUpdate, SyncSettings,
};

/// Version understood by this runtime protocol implementation.
pub const CORE_PROTOCOL_VERSION: u32 = 1;

/// Identifier supplied by a caller for idempotent command submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandId(pub u64);

/// Identifier assigned by the runtime to a mutating operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(pub u64);

/// A versioned command request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreCommandRequest {
    pub protocol_version: u32,
    pub command_id: CommandId,
    pub command: CoreCommand,
}

/// Library synchronization mode requested through the runtime shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SyncKind {
    Full,
    Incremental,
    FullReconcile,
}

/// Commands currently owned by the runtime shell.
///
/// Audio-output and background-job ownership intentionally remain outside this
/// enum until their adapters move into the runtime in later migration phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CoreCommand {
    Initialize,
    GetSnapshot,
    Connect {
        params: ConnectParams,
    },
    UpdateServerSettings {
        update: ServerSettingsUpdate,
    },
    RestoreSession,
    Disconnect,
    GetConnectionStatus,
    GetSyncSettings,
    SetSyncSettings {
        settings: SyncSettings,
    },
    GetConnectivitySettings,
    SetConnectivity {
        settings: ConnectivitySettings,
    },
    StartSync {
        kind: SyncKind,
    },
    RunDueLibrarySync,
    GetScanStatus,
    StartScan,
    GetLibrarySyncStatus,
    GetArtists,
    GetAlbums {
        artist_id: Option<String>,
    },
    GetSongs {
        album_id: Option<String>,
        artist_id: Option<String>,
    },
    GetAlbumList {
        list_type: String,
        size: Option<usize>,
        offset: Option<usize>,
    },
    SearchLibrary {
        query: String,
        limit: Option<usize>,
    },
    GetPlaylists,
    GetPlaylistSongs {
        playlist_id: String,
    },
    CreatePlaylist {
        name: String,
        song_ids: Vec<String>,
    },
    RenamePlaylist {
        playlist_id: String,
        name: String,
    },
    DeletePlaylist {
        playlist_id: String,
    },
    AddSongsToPlaylist {
        playlist_id: String,
        song_ids: Vec<String>,
    },
    RemoveSongsFromPlaylist {
        playlist_id: String,
        song_indexes: Vec<i64>,
    },
    GetCoverArtUri {
        id: String,
        size: Option<i32>,
    },
    GetSongCoverArtUri {
        id: String,
        size: Option<i32>,
    },
    GetStreamUri {
        song_id: String,
    },
    GetAudioCacheStats,
    GetOfflineSongIds,
    SetMaxCacheSize {
        max_size: u64,
    },
    ClearAudioCache,
    IsSongCached {
        song_id: String,
    },
    DownloadSong {
        song_id: String,
    },
    RemoveCachedSong {
        song_id: String,
    },
    DownloadAlbum {
        album_id: String,
    },
    DownloadPlaylist {
        playlist_id: String,
    },
    SetPlaylistSavedOffline {
        playlist_id: String,
        saved_offline: bool,
    },
    ReconcileSavedPlaylistsOffline,
    GetPlaybackState,
    SavePlaybackPosition {
        progress: PlaybackProgress,
    },
    GetLastfmStatus,
    BeginLastfmAuth,
    CompleteLastfmAuth,
    DisconnectLastfm,
    GetLastfmQueue,
    RetryLastfmQueue,
    GetAudioProcessingSettings,
    SetAudioProcessing {
        settings: AudioProcessingSettings,
    },
    ExportPortableBackup {
        path: String,
    },
    ImportPortableBackup {
        path: String,
    },
    GetQueue,
    PlaySelection {
        song_id: String,
        song_ids: Vec<String>,
    },
    AddToQueue {
        item: QueueItem,
    },
    AddSongsToQueue {
        items: Vec<QueueItem>,
    },
    InsertNext {
        item: QueueItem,
    },
    InsertNextSongs {
        items: Vec<QueueItem>,
    },
    RemoveFromQueue {
        index: usize,
    },
    ClearQueue,
    MoveQueueItem {
        from: usize,
        to: usize,
    },
    PlayQueueItem {
        index: usize,
    },
    PlayNext {
        force: Option<bool>,
    },
    PlayPrevious,
    ToggleShuffle,
    SetRepeatMode {
        mode: RepeatMode,
    },
    CycleRepeatMode,
    RerollNext,
    Shutdown,
}

impl CoreCommand {
    #[must_use]
    pub(crate) fn is_mutation(&self) -> bool {
        !matches!(
            self,
            Self::Initialize
                | Self::GetSnapshot
                | Self::GetConnectionStatus
                | Self::GetSyncSettings
                | Self::GetConnectivitySettings
                | Self::GetScanStatus
                | Self::GetLibrarySyncStatus
                | Self::GetArtists
                | Self::GetAlbums { .. }
                | Self::GetSongs { .. }
                | Self::GetAlbumList { .. }
                | Self::SearchLibrary { .. }
                | Self::GetPlaylists
                | Self::GetPlaylistSongs { .. }
                | Self::GetCoverArtUri { .. }
                | Self::GetSongCoverArtUri { .. }
                | Self::GetStreamUri { .. }
                | Self::GetAudioCacheStats
                | Self::GetOfflineSongIds
                | Self::IsSongCached { .. }
                | Self::GetPlaybackState
                | Self::GetLastfmStatus
                | Self::GetLastfmQueue
                | Self::GetAudioProcessingSettings
                | Self::GetQueue
        )
    }

    #[must_use]
    pub(crate) fn changes_library(&self) -> bool {
        matches!(
            self,
            Self::StartSync { .. }
                | Self::RunDueLibrarySync
                | Self::CreatePlaylist { .. }
                | Self::RenamePlaylist { .. }
                | Self::DeletePlaylist { .. }
                | Self::AddSongsToPlaylist { .. }
                | Self::RemoveSongsFromPlaylist { .. }
                | Self::SetPlaylistSavedOffline { .. }
                | Self::ImportPortableBackup { .. }
        )
    }

    #[must_use]
    pub(crate) fn changes_settings(&self) -> bool {
        matches!(
            self,
            Self::SetSyncSettings { .. }
                | Self::SetConnectivity { .. }
                | Self::SetMaxCacheSize { .. }
                | Self::SetAudioProcessing { .. }
                | Self::ImportPortableBackup { .. }
        )
    }
}

/// Runtime lifecycle exposed in snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeLifecycle {
    Starting,
    Ready,
    ShuttingDown,
    Faulted,
}

/// Connectivity projection known by the serialized runtime shell.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum ConnectivityState {
    Unconfigured,
    OfflineManual {
        server_url: Option<String>,
        username: Option<String>,
    },
    Disconnected {
        server_url: String,
        username: String,
    },
    Online {
        server_url: String,
        username: String,
        server_version: Option<String>,
    },
}

/// Aggregate download projection used for reconciliation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSnapshot {
    pub downloading_song_ids: Vec<String>,
    pub offline_song_ids: Vec<String>,
}

/// Complete operational projection available after missed events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreSnapshot {
    pub protocol_version: u32,
    pub revision: u64,
    pub lifecycle: RuntimeLifecycle,
    pub connectivity: ConnectivityState,
    pub playback: PlaybackState,
    pub queue: QueueState,
    pub sync: LibrarySyncStatus,
    pub downloads: DownloadSnapshot,
    pub settings_revision: u64,
    pub library_revision: u64,
    pub last_failure: Option<OperationFailure>,
}

/// Stable error categories at protocol boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolErrorCode {
    UnsupportedProtocolVersion,
    InvalidCommandId,
    InvalidInput,
    NotConnected,
    OfflineMode,
    Conflict,
    RuntimeUnavailable,
    Persistence,
    Network,
    Internal,
}

/// Structured protocol failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolError {
    pub code: ProtocolErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl ProtocolError {
    #[must_use]
    pub fn new(code: ProtocolErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }
}

impl From<&CoreError> for ProtocolError {
    fn from(error: &CoreError) -> Self {
        let (code, retryable) = match error {
            CoreError::NotConnected => (ProtocolErrorCode::NotConnected, true),
            CoreError::OfflineMode => (ProtocolErrorCode::OfflineMode, false),
            CoreError::InvalidInput(_) | CoreError::InvalidAlbumListType(_) => {
                (ProtocolErrorCode::InvalidInput, false)
            }
            CoreError::Database(_) | CoreError::Io(_) | CoreError::Serde(_) => {
                (ProtocolErrorCode::Persistence, false)
            }
            CoreError::Subsonic(_) | CoreError::Lastfm(_) => (ProtocolErrorCode::Network, true),
            CoreError::LockPoisoned => (ProtocolErrorCode::Internal, false),
        };
        Self::new(code, error.to_string(), retryable)
    }
}

/// Failure retained in a snapshot for post-suspension diagnosis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationFailure {
    pub command_id: CommandId,
    pub operation_id: Option<OperationId>,
    pub error: ProtocolError,
}

/// Command completion status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Succeeded,
    Failed,
}

/// Deterministic response to a command request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreCommandResult {
    pub protocol_version: u32,
    pub command_id: CommandId,
    pub accepted_revision: u64,
    pub operation_id: Option<OperationId>,
    pub status: CommandStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ProtocolError>,
}

impl CoreCommandResult {
    #[must_use]
    pub fn succeeded(
        command_id: CommandId,
        revision: u64,
        operation_id: Option<OperationId>,
        value: Value,
    ) -> Self {
        Self {
            protocol_version: CORE_PROTOCOL_VERSION,
            command_id,
            accepted_revision: revision,
            operation_id,
            status: CommandStatus::Succeeded,
            value: Some(value),
            error: None,
        }
    }

    #[must_use]
    pub fn failed(
        command_id: CommandId,
        revision: u64,
        operation_id: Option<OperationId>,
        error: ProtocolError,
    ) -> Self {
        Self {
            protocol_version: CORE_PROTOCOL_VERSION,
            command_id,
            accepted_revision: revision,
            operation_id,
            status: CommandStatus::Failed,
            value: None,
            error: Some(error),
        }
    }
}

/// Ordered event emitted before a mutating command reports completion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreEvent {
    pub protocol_version: u32,
    pub stream_id: u64,
    pub event_id: u64,
    pub revision: u64,
    pub cause_command_id: CommandId,
    pub operation_id: Option<OperationId>,
    pub kind: CoreEventKind,
}

/// Runtime event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CoreEventKind {
    SnapshotChanged { snapshot: Box<CoreSnapshot> },
    OperationFailed { failure: OperationFailure },
    RuntimeShuttingDown,
}
