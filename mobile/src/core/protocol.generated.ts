/* eslint-disable */
// @generated from crates/stereodrome-core/src/protocol.rs. Do not hand-edit.

import type {
  Album,
  AlbumListEntry,
  Artist,
  AudioProcessingSettings,
  BackupSummary,
  CacheStats,
  ConnectionStatus,
  ConnectivitySettings,
  DownloadStatus,
  LastfmAuthStart,
  LastfmQueueItem,
  LastfmStatus,
  LibrarySyncStatus,
  PlaybackProgress,
  PlaybackStateSnapshot,
  Playlist,
  QueueItem,
  QueueState,
  RepeatMode,
  SavedPlaylistOfflineResult,
  SavedPlaylistOfflineStatus,
  ScanStatus,
  SearchResults,
  Song,
  SyncSettings,
} from "@/types/music";

export const CORE_PROTOCOL_VERSION = 1 as const;

export type RuntimeLifecycle =
  | "starting"
  | "ready"
  | "shutting-down"
  | "faulted";

export type PlatformLifecycle = "foreground" | "background";

export type ConnectivityState =
  | { status: "unconfigured" }
  | {
      status: "offline-manual";
      server_url: string | null;
      username: string | null;
    }
  | { status: "disconnected"; server_url: string; username: string }
  | {
      status: "online";
      server_url: string;
      username: string;
      server_version: string | null;
    };

export type PlaybackProjectionSong = {
  id: string;
  title: string;
  artist: string;
  album: string;
  duration_seconds: number;
  artwork_uri: string | null;
};

export type PlaybackProjection = {
  state: "playing" | "paused" | "stopped" | "stalled";
  is_playing: boolean;
  audio_loaded: boolean;
  output_state: "closed" | "ready" | "failed" | "unavailable";
  song: PlaybackProjectionSong | null;
  position_seconds: number;
  duration_seconds: number;
  volume: number;
  queue: QueueState;
  queue_index: number | null;
  queue_length: number;
  can_play: boolean;
  can_next: boolean;
  can_previous: boolean;
  can_seek: boolean;
  preparing_operation_id: number | null;
};

export type ProtocolError = {
  code:
    | "unsupported_protocol_version"
    | "invalid_command_id"
    | "invalid_input"
    | "not_connected"
    | "offline_mode"
    | "conflict"
    | "runtime_unavailable"
    | "cancelled"
    | "persistence"
    | "network"
    | "audio"
    | "internal";
  message: string;
  retryable: boolean;
};

export type OperationFailure = {
  command_id: number;
  operation_id: number | null;
  error: ProtocolError;
};

export type CoreSnapshot = {
  protocol_version: typeof CORE_PROTOCOL_VERSION;
  revision: number;
  lifecycle: RuntimeLifecycle;
  connectivity: ConnectivityState;
  playback: PlaybackProjection;
  queue: QueueState;
  sync: LibrarySyncStatus;
  downloads: {
    downloading_song_ids: string[];
    offline_song_ids: string[];
  };
  operations: Array<{
    operation_id: number;
    cause_command_id: number;
    kind: unknown;
    phase: "running" | "cancelling";
  }>;
  saved_playlist_offline: {
    running: boolean;
    operation_id: number | null;
    last_error: string | null;
  };
  platform_lifecycle: PlatformLifecycle;
  network_available: boolean;
  settings_revision: number;
  library_revision: number;
  last_failure: OperationFailure | null;
};

export type CoreCommand =
  | { type: "initialize" }
  | { type: "get-snapshot" }
  | {
      type: "connect";
      params: { url: string; username: string; password: string };
    }
  | {
      type: "update-server-settings";
      update: { url?: string; username?: string };
    }
  | { type: "restore-session" }
  | { type: "disconnect" }
  | { type: "get-connection-status" }
  | { type: "get-sync-settings" }
  | { type: "set-sync-settings"; settings: SyncSettings }
  | { type: "get-connectivity-settings" }
  | { type: "set-connectivity"; settings: ConnectivitySettings }
  | { type: "report-network"; available: boolean }
  | { type: "report-lifecycle"; lifecycle: PlatformLifecycle }
  | { type: "start-sync"; kind: "full" | "incremental" | "full-reconcile" }
  | { type: "run-background-tick" }
  | { type: "cancel-operation"; operation_id: number }
  | { type: "run-due-library-sync" }
  | { type: "get-scan-status" }
  | { type: "start-scan" }
  | { type: "get-now-playing" }
  | { type: "get-library-sync-status" }
  | { type: "get-artists" }
  | { type: "get-albums"; artist_id: string | null }
  | { type: "get-songs"; album_id: string | null; artist_id: string | null }
  | {
      type: "get-album-list";
      list_type: string;
      size: number | null;
      offset: number | null;
    }
  | { type: "search-library"; query: string; limit: number | null }
  | { type: "get-playlists" }
  | { type: "get-playlist-songs"; playlist_id: string }
  | { type: "create-playlist"; name: string; song_ids: string[] }
  | { type: "rename-playlist"; playlist_id: string; name: string }
  | { type: "delete-playlist"; playlist_id: string }
  | { type: "add-songs-to-playlist"; playlist_id: string; song_ids: string[] }
  | {
      type: "remove-songs-from-playlist";
      playlist_id: string;
      song_indexes: number[];
    }
  | { type: "get-cover-art-uri"; id: string; size: number | null }
  | { type: "get-song-cover-art-uri"; id: string; size: number | null }
  | { type: "get-stream-uri"; song_id: string }
  | { type: "get-audio-cache-stats" }
  | { type: "get-offline-song-ids" }
  | { type: "set-max-cache-size"; max_size: number }
  | { type: "clear-audio-cache" }
  | { type: "is-song-cached"; song_id: string }
  | { type: "download-song"; song_id: string }
  | { type: "remove-cached-song"; song_id: string }
  | { type: "download-album"; album_id: string }
  | { type: "download-playlist"; playlist_id: string }
  | {
      type: "set-playlist-saved-offline";
      playlist_id: string;
      saved_offline: boolean;
    }
  | { type: "reconcile-saved-playlists-offline" }
  | { type: "start-saved-playlists-offline-reconcile" }
  | { type: "get-saved-playlists-offline-status" }
  | { type: "start-queue-prefetch"; reserve_first?: boolean }
  | { type: "cancel-queue-prefetch"; invalidate_completed?: boolean }
  | { type: "get-playback-state" }
  | { type: "save-playback-position"; progress: PlaybackProgress }
  | { type: "get-lastfm-status" }
  | { type: "begin-lastfm-auth" }
  | { type: "complete-lastfm-auth" }
  | { type: "disconnect-lastfm" }
  | { type: "get-lastfm-queue" }
  | { type: "retry-lastfm-queue" }
  | { type: "get-audio-processing-settings" }
  | { type: "set-audio-processing"; settings: AudioProcessingSettings }
  | { type: "export-portable-backup"; path: string }
  | { type: "import-portable-backup"; path: string }
  | { type: "get-queue" }
  | { type: "play-selection"; song_id: string; song_ids: string[] }
  | { type: "clear-playback" }
  | {
      type: "navigate-playback";
      navigation:
        | { type: "index"; index: number }
        | { type: "next"; force: boolean }
        | { type: "previous" };
    }
  | { type: "toggle-playback" }
  | { type: "pause-playback" }
  | { type: "resume-playback" }
  | { type: "stop-playback" }
  | { type: "seek-to"; seconds: number }
  | { type: "seek-by"; seconds: number }
  | { type: "set-playback-volume"; volume: number }
  | { type: "rebuild-audio-output" }
  | { type: "apply-audio-settings" }
  | { type: "prepare-next-transition" }
  | {
      type: "report-platform-playback";
      event:
        | { type: "audio-focus-lost"; transient: boolean }
        | { type: "audio-focus-gained" }
        | { type: "interruption-began" }
        | { type: "interruption-ended"; should_resume: boolean }
        | { type: "route-lost" }
        | { type: "media-services-reset" };
    }
  | { type: "add-to-queue"; item: QueueItem }
  | { type: "add-songs-to-queue"; items: QueueItem[] }
  | { type: "insert-next"; item: QueueItem }
  | { type: "insert-next-songs"; items: QueueItem[] }
  | { type: "remove-from-queue"; index: number }
  | { type: "clear-queue" }
  | { type: "move-queue-item"; from: number; to: number }
  | { type: "play-queue-item"; index: number }
  | { type: "play-next"; force: boolean | null }
  | { type: "play-previous" }
  | { type: "toggle-shuffle" }
  | { type: "set-repeat-mode"; mode: RepeatMode }
  | { type: "cycle-repeat-mode" }
  | { type: "reroll-next" }
  | { type: "shutdown" };

export type CoreCommandValue = {
  "get-connection-status": ConnectionStatus;
  "get-sync-settings": SyncSettings;
  "set-sync-settings": SyncSettings;
  "get-connectivity-settings": ConnectivitySettings;
  "set-connectivity": ConnectivitySettings;
  "run-due-library-sync": string | null;
  "get-scan-status": ScanStatus;
  "start-scan": ScanStatus;
  "get-library-sync-status": LibrarySyncStatus;
  "get-artists": Artist[];
  "get-albums": Album[];
  "get-songs": Song[];
  "get-album-list": AlbumListEntry[];
  "search-library": SearchResults;
  "get-playlists": Playlist[];
  "get-playlist-songs": Song[];
  "create-playlist": Playlist;
  "get-cover-art-uri": string;
  "get-song-cover-art-uri": string | null;
  "get-stream-uri": string;
  "get-audio-cache-stats": CacheStats;
  "get-offline-song-ids": string[];
  "set-max-cache-size": CacheStats;
  "clear-audio-cache": CacheStats;
  "is-song-cached": DownloadStatus;
  "download-song": DownloadStatus;
  "remove-cached-song": DownloadStatus;
  "download-album": DownloadStatus[];
  "download-playlist": DownloadStatus[];
  "set-playlist-saved-offline": SavedPlaylistOfflineResult;
  "reconcile-saved-playlists-offline": SavedPlaylistOfflineResult[];
  "get-saved-playlists-offline-status": SavedPlaylistOfflineStatus;
  "get-playback-state": PlaybackStateSnapshot;
  "get-lastfm-status": LastfmStatus;
  "begin-lastfm-auth": LastfmAuthStart;
  "complete-lastfm-auth": LastfmStatus;
  "disconnect-lastfm": LastfmStatus;
  "get-lastfm-queue": LastfmQueueItem[];
  "retry-lastfm-queue": number;
  "get-audio-processing-settings": AudioProcessingSettings;
  "set-audio-processing": AudioProcessingSettings;
  "export-portable-backup": BackupSummary;
  "import-portable-backup": BackupSummary;
  "get-queue": QueueState;
};

export type CoreCommandRequest = {
  protocol_version: typeof CORE_PROTOCOL_VERSION;
  command_id: number;
  command: CoreCommand;
};

export type CoreCommandResult<T = unknown> =
  | {
      protocol_version: typeof CORE_PROTOCOL_VERSION;
      command_id: number;
      accepted_revision: number;
      operation_id: number | null;
      status: "succeeded";
      value: T;
    }
  | {
      protocol_version: typeof CORE_PROTOCOL_VERSION;
      command_id: number;
      accepted_revision: number;
      operation_id: number | null;
      status: "failed";
      error: ProtocolError;
    };

export type CoreEvent = {
  protocol_version: typeof CORE_PROTOCOL_VERSION;
  stream_id: number;
  event_id: number;
  revision: number;
  cause_command_id: number;
  operation_id: number | null;
  kind:
    | { type: "snapshot-changed"; snapshot: CoreSnapshot }
    | { type: "operation-failed"; failure: OperationFailure }
    | { type: "runtime-shutting-down" };
};
