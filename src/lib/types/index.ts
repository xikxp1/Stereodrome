export type {
  Album,
  AlbumListEntry,
  Artist,
  BackupSummary,
  CacheStats,
  ConnectParams,
  ConnectionStatus,
  ConnectivitySettings,
  LastfmAuthStart,
  LastfmQueueItem,
  LastfmStatus,
  LibrarySyncStatus,
  NowPlayingEntry,
  Playlist,
  QueueItem,
  QueueState,
  RepeatMode,
  SavedPlaylistOfflineResult,
  ScanStatus,
  SearchResultAlbum,
  SearchResultArtist,
  SearchResultSong,
  SearchResults,
  Song,
  SyncJobStatus,
  SyncResult,
  SyncSettings,
} from "./protocol.generated";

import type { NowPlayingEntry, Song } from "./protocol.generated";

// Playback types
export interface PlaybackState {
  current_song: Song | null;
  is_playing: boolean;
  position: number;
  duration: number;
  volume: number;
}

// Normalization types
export type DynamicsPreset = "light" | "medium" | "heavy";
export type BinauralPreset = "default" | "cmoy" | "jmeier" | "aggressive";

export interface NormalizationSettings {
  enabled: boolean;
  mode: "track" | "album";
  target_lufs: number;
  pre_amp_db: number;
  prevent_clipping: boolean;
  dynamics_enabled: boolean;
  dynamics_preset: DynamicsPreset;
}

export interface NormalizationStats {
  analyzed_count: number;
  total_count: number;
}

export interface AnalysisProgress {
  analyzed: number;
  total: number;
  current_song: string;
  analyzed_count: number;
  total_count: number;
}

export interface PlaybackSettings {
  gapless_enabled: boolean;
  crossfade_enabled: boolean;
  crossfade_on_manual_queue_advance: boolean;
  crossfade_duration_ms: number;
  binaural_enabled: boolean;
  binaural_preset: BinauralPreset;
  equalizer_enabled: boolean;
  equalizer_bands_db: number[];
  show_next_song_in_miniplayer: boolean;
  prefetch_count: number;
}

export interface NotificationSettings {
  enabled: boolean;
  notify_when_focused: boolean;
  notify_when_miniplayer_open: boolean;
}

export interface CacheLocationInfo {
  cache_root: string;
  default_cache_root: string;
  audio_cache_dir: string;
  cover_cache_dir: string;
  is_default: boolean;
}

export interface CacheMoveSummary {
  moved_files: number;
  skipped_files: number;
  failed_files: number;
}

export interface CacheRootUpdateResult {
  locations: CacheLocationInfo;
  audio: CacheMoveSummary;
  cover_art: CacheMoveSummary;
}

export interface SendNowPlayingNotificationParams {
  title: string;
  body: string;
  cover_art_path?: string | null;
}

export type SyncJobKind = "incremental" | "full_reconcile";

export interface LibraryContentUpdatedEvent {
  job: SyncJobKind;
  new_artists: number;
  new_albums: number;
  new_songs: number;
  has_new_items: boolean;
}

export interface SystemTimePreferences {
  use_24_hour_clock: boolean;
  locale: string | null;
}

export interface MiniPlayerPosition {
  x: number;
  y: number;
}

export type MiniPlayerMode = "mini" | "nano";

export interface MiniPlayerHoverState {
  hovered: boolean;
}

export interface NowPlayingEvent {
  entries: NowPlayingEntry[];
}
