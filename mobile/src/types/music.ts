export type ConnectionStatus = {
  connected: boolean;
  server_url: string | null;
  username: string | null;
  server_version: string | null;
};

export type BackupSummary = {
  artists: number;
  albums: number;
  songs: number;
  playlists: number;
  queue_items: number;
};

export type ConnectivitySettings = {
  manual_offline_enabled: boolean;
};

export type Artist = {
  id: string;
  name: string;
  album_count: number;
  cover_art_id: string | null;
  synced_at: string;
};

export type Album = {
  id: string;
  artist_id: string;
  name: string;
  year: number | null;
  song_count: number;
  duration: number | null;
  cover_art_id: string | null;
  synced_at: string;
  artist_name: string | null;
};

export type AlbumListEntry = {
  id: string;
  name: string;
  artist_id: string | null;
  artist_name: string | null;
  year: number | null;
  song_count: number | null;
  duration: number | null;
  cover_art_id: string | null;
  play_count: number | null;
  created: string | null;
};

export type Song = {
  id: string;
  album_id: string;
  artist_id: string;
  title: string;
  track_number: number | null;
  disc_number: number;
  duration: number | null;
  bit_rate: number | null;
  size: number | null;
  suffix: string | null;
  content_type: string | null;
  path: string | null;
  year: number | null;
  genre: string | null;
  synced_at: string;
  artist: string | null;
  album: string | null;
};

export type Playlist = {
  id: string;
  name: string;
  song_count: number;
  duration: number;
  owner: string | null;
  cover_art_id: string | null;
  created_at: string;
  changed_at: string;
  saved_offline: boolean;
  offline_saved_at: string | null;
};

export type SearchResultSong = {
  id: string;
  title: string;
  artist: string | null;
  album: string | null;
  duration: number | null;
};

export type SearchResultAlbum = {
  id: string;
  name: string;
  artist: string | null;
  year: number | null;
  song_count: number;
};

export type SearchResultArtist = {
  id: string;
  name: string;
  album_count: number;
};

export type SearchResults = {
  songs: SearchResultSong[];
  albums: SearchResultAlbum[];
  artists: SearchResultArtist[];
};

export type PlayableSong = Pick<
  Song,
  "id" | "title" | "artist" | "album" | "duration"
>;

export type RepeatMode = "Off" | "All" | "One";

export type QueueItem = {
  song_id: string;
  title: string;
  artist: string;
  album: string;
  duration: number;
};

export type QueueState = {
  items: QueueItem[];
  current_index: number | null;
  shuffle: boolean;
  repeat_mode: RepeatMode;
  pending_navigation_index: number | null;
  prepared_next_item: QueueItem | null;
};

export type SyncResult = {
  artists: number;
  albums: number;
  songs: number;
};

export type SyncSettings = {
  incremental_enabled: boolean;
  incremental_interval_minutes: number;
  full_reconcile_enabled: boolean;
  full_reconcile_interval_hours: number;
};

export type ScanStatus = {
  scanning: boolean;
  count: number | null;
};

export type SyncJobStatus = {
  enabled: boolean;
  interval_minutes: number;
  running: boolean;
  last_attempt_at: string | null;
  last_success_at: string | null;
  last_error: string | null;
  next_run_at: string | null;
};

export type LibrarySyncStatus = {
  active_job: string | null;
  full: SyncJobStatus;
  incremental: SyncJobStatus;
  full_reconcile: SyncJobStatus;
};

export type CacheStats = {
  total_size: number;
  file_count: number;
  max_size: number;
};

export type SongFileState = "downloaded" | "downloading" | "not_downloaded";

export type DownloadStatus = {
  song_id: string;
  cached: boolean;
  path: string | null;
  bytes: number;
};

export type SavedPlaylistOfflineResult = {
  playlist_id: string;
  saved_offline: boolean;
  downloaded_count: number;
  removed_count: number;
  skipped_protected_count: number;
};

export type SavedPlaylistOfflineStatus = {
  running: boolean;
  last_error: string | null;
};

export type PlaybackStateSnapshot = {
  current_song_id: string | null;
  position_seconds: number;
  duration_seconds: number;
  was_playing: boolean;
  app_volume: number;
  updated_at: string;
};

export type PlaybackSnapshotSong = {
  id: string;
  title: string;
  artist: string;
  album: string;
  duration_seconds: number;
  artwork_uri: string | null;
};

export type PlaybackSnapshot = {
  seq: number;
  state: "playing" | "paused" | "stopped" | "stalled";
  is_playing: boolean;
  audio_loaded: boolean;
  output_state: "closed" | "ready" | "failed" | "unavailable";
  song: PlaybackSnapshotSong | null;
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
};

export type FileStateSnapshot = {
  seq: number;
  downloaded_song_ids: string[];
  downloading_song_ids: string[];
};

export type PlaybackProgress = {
  song_id: string;
  position_seconds: number;
  duration_seconds: number;
  is_playing: boolean;
};

export type LastfmStatus = {
  available: boolean;
  authenticated: boolean;
  enabled: boolean;
  username: string | null;
  pending_auth: boolean;
  queue_count: number;
  last_error: string | null;
};

export type LastfmAuthStart = {
  auth_url: string;
};

export type LastfmQueueItem = {
  id: number;
  song_id: string;
  title: string;
  artist: string;
  album: string | null;
  duration: number | null;
  played_at: number;
  attempts: number;
  next_retry_at: number;
  last_error: string | null;
  created_at: string;
  updated_at: string;
};

export type AudioPlaybackStatus = {
  state: "playing" | "paused" | "stopped" | "stalled";
  is_playing: boolean;
  current_song_id: string | null;
  position: number;
  duration: number;
  volume: number;
};

export type AudioProcessingSettings = {
  normalization_enabled: boolean;
  normalization_mode: "track" | "album";
  target_lufs: number;
  preamp_db: number;
  prevent_clipping: boolean;
  dynamics_enabled: boolean;
  dynamics_preset: "light" | "medium" | "heavy";
  binaural_enabled: boolean;
  binaural_preset: "light" | "medium" | "strong";
  equalizer_enabled: boolean;
  equalizer_bands_db: number[];
  gapless_enabled: boolean;
  crossfade_enabled: boolean;
  crossfade_duration_ms: number;
  prefetch_count: number;
};
