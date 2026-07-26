export type {
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
  PlaybackState,
  Playlist,
  QueueItem,
  QueueState,
  RepeatMode,
  SavedPlaylistOfflineResult,
  SavedPlaylistOfflineStatus,
  ScanStatus,
  SearchResultAlbum,
  SearchResultArtist,
  SearchResultSong,
  SearchResults,
  Song,
  SyncJobStatus,
  SyncResult,
  SyncSettings,
} from "@/core/protocol.generated";

import type { Song } from "@/core/protocol.generated";

export type PlayableSong = Pick<
  Song,
  "id" | "title" | "artist" | "album" | "duration"
>;

export type SongFileState = "downloaded" | "downloading" | "not_downloaded";
