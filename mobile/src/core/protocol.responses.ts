// Hand-written: the runtime erases command results to `serde_json::Value`
// (see `runtime::effect::execute`), so the command -> payload mapping is not
// derivable from the Rust types and cannot be generated.
// Keep in sync with `crates/stereodrome-core/src/runtime/effect.rs`.
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
  PlaybackState,
  Playlist,
  QueueState,
  SavedPlaylistOfflineResult,
  SavedPlaylistOfflineStatus,
  ScanStatus,
  SearchResults,
  Song,
  SyncSettings,
} from "@/core/protocol.generated";

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
  "get-playback-state": PlaybackState;
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
