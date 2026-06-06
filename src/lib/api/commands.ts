import { invoke } from "@tauri-apps/api/core";
import type {
  ConnectionStatus,
  ConnectParams,
  Artist,
  Album,
  AlbumListEntry,
  Song,
  SyncResult,
  ScanStatus,
  PlaybackState,
  SearchResults,
  NormalizationSettings,
  NormalizationStats,
  AnalysisProgress,
  PlaybackSettings,
  NotificationSettings,
  SendNowPlayingNotificationParams,
  SyncSettings,
  LibrarySyncStatus,
  SystemTimePreferences,
  LastfmAuthStart,
  LastfmQueueItem,
  LastfmStatus,
  SavedPlaylistOfflineResult,
  MiniPlayerMode,
  MiniPlayerPosition,
} from "$lib/types";

// Auth commands
export async function connectServer(
  params: ConnectParams
): Promise<ConnectionStatus> {
  return invoke<ConnectionStatus>("connect_server", { params });
}

export async function disconnectServer(): Promise<void> {
  return invoke("disconnect_server");
}

export async function getConnectionStatus(): Promise<ConnectionStatus> {
  return invoke<ConnectionStatus>("get_connection_status");
}

export async function restoreSession(): Promise<ConnectionStatus> {
  return invoke<ConnectionStatus>("restore_session");
}

// Library commands
export async function syncLibrary(): Promise<SyncResult> {
  return invoke<SyncResult>("sync_library");
}

export async function reconcileLibraryState(): Promise<SyncResult> {
  return invoke<SyncResult>("reconcile_library_state");
}

export async function getLibrarySyncStatus(): Promise<LibrarySyncStatus> {
  return invoke<LibrarySyncStatus>("get_library_sync_status");
}

export async function getArtists(): Promise<Artist[]> {
  return invoke<Artist[]>("get_artists");
}

export async function getAlbumCount(): Promise<number> {
  return invoke<number>("get_album_count");
}

export async function getAlbums(artistId?: string): Promise<Album[]> {
  return invoke<Album[]>("get_albums", { artistId });
}

export async function getAlbumList(
  listType: string,
  size?: number,
  offset?: number
): Promise<AlbumListEntry[]> {
  return invoke<AlbumListEntry[]>("get_album_list", { listType, size, offset });
}

export async function getSongs(
  albumId?: string,
  artistId?: string
): Promise<Song[]> {
  return invoke<Song[]>("get_songs", { albumId, artistId });
}

export async function removeSongsFromPlaylist(
  playlistId: string,
  positions: number[]
): Promise<void> {
  return invoke("remove_songs_from_playlist", { playlistId, positions });
}

export async function setPlaylistSavedOffline(
  playlistId: string,
  savedOffline: boolean
): Promise<SavedPlaylistOfflineResult> {
  return invoke<SavedPlaylistOfflineResult>("set_playlist_saved_offline", {
    playlistId,
    savedOffline,
  });
}

export async function reconcileSavedPlaylistsOffline(): Promise<
  SavedPlaylistOfflineResult[]
> {
  return invoke<SavedPlaylistOfflineResult[]>(
    "reconcile_saved_playlists_offline"
  );
}

// Playback commands
export async function playSong(songId: string): Promise<void> {
  return invoke("play_song", { songId });
}

export async function pausePlayback(): Promise<void> {
  return invoke("pause_playback");
}

export async function resumePlayback(): Promise<void> {
  return invoke("resume_playback");
}

export async function stopPlayback(): Promise<void> {
  return invoke("stop_playback");
}

export async function setVolume(volume: number): Promise<void> {
  return invoke("set_volume", { volume });
}

export async function setPersistedVolume(volume: number): Promise<void> {
  return invoke("set_persisted_volume", { volume });
}

export async function seekPlayback(position: number): Promise<void> {
  return invoke("seek_playback", { position });
}

export async function getPlaybackStatus(): Promise<PlaybackState> {
  return invoke<PlaybackState>("get_playback_status");
}

// Queue commands
export async function playSongWithQueue(
  songId: string,
  songIds: string[]
): Promise<void> {
  return invoke("play_song_with_queue", { songId, songIds });
}

export async function rerollNextQueueItem(): Promise<boolean> {
  return invoke<boolean>("reroll_next_queue_item");
}

// Search commands
export async function searchLibrary(
  query: string,
  limit?: number
): Promise<SearchResults> {
  return invoke<SearchResults>("search_library", { query, limit });
}

// Cover art commands
export async function getCoverArt(
  coverArtId: string,
  size?: number
): Promise<string> {
  return invoke<string>("get_cover_art", { coverArtId, size });
}

export async function getSongCoverArt(
  songId: string,
  size?: number
): Promise<string | null> {
  return invoke<string | null>("get_song_cover_art", { songId, size });
}

export async function getCoverArtPath(
  coverArtId: string,
  size?: number
): Promise<string> {
  return invoke<string>("get_cover_art_path", { coverArtId, size });
}

export async function openMiniPlayer(
  position: MiniPlayerPosition
): Promise<void> {
  return invoke("open_mini_player", { position });
}

export async function closeMiniPlayer(): Promise<void> {
  return invoke("close_mini_player");
}

export async function restoreMainWindow(): Promise<void> {
  return invoke("restore_main_window");
}

export async function getMiniPlayerPosition(): Promise<MiniPlayerPosition | null> {
  return invoke<MiniPlayerPosition | null>("get_mini_player_position");
}

export async function setMiniPlayerPosition(
  position: MiniPlayerPosition
): Promise<void> {
  return invoke("set_mini_player_position", { position });
}

export async function setMiniPlayerMode(
  mode: MiniPlayerMode,
  position: MiniPlayerPosition
): Promise<void> {
  return invoke("set_mini_player_mode", { mode, position });
}

// Audio cache commands
export interface CacheStats {
  total_size: number;
  file_count: number;
  max_size: number;
}

// Cache size limits (must match Rust constants)
export const MIN_CACHE_SIZE = 500 * 1024 * 1024; // 500 MB
export const MAX_CACHE_SIZE = 50 * 1024 * 1024 * 1024; // 50 GB
export const DEFAULT_CACHE_SIZE = 5 * 1024 * 1024 * 1024; // 5 GB

export async function getAudioCacheStats(): Promise<CacheStats> {
  return invoke<CacheStats>("get_audio_cache_stats");
}

export async function getOfflineSongIds(): Promise<string[]> {
  return invoke<string[]>("get_offline_song_ids");
}

export async function clearAudioCache(): Promise<void> {
  return invoke("clear_audio_cache");
}

export async function setMaxCacheSize(size: number): Promise<CacheStats> {
  return invoke<CacheStats>("set_max_cache_size", { size });
}

// Scrobbling commands
export async function scrobbleSubmit(songId: string): Promise<void> {
  return invoke("scrobble_submit", { songId });
}

// Scan commands
export async function getScanStatus(): Promise<ScanStatus> {
  return invoke<ScanStatus>("get_scan_status");
}

export async function startScan(): Promise<ScanStatus> {
  return invoke<ScanStatus>("start_scan");
}

// Normalization commands
export async function getNormalizationSettings(): Promise<NormalizationSettings> {
  return invoke<NormalizationSettings>("get_normalization_settings");
}

export async function setNormalizationSettings(
  settings: NormalizationSettings
): Promise<void> {
  return invoke("set_normalization_settings", { settings });
}

export async function getNormalizationStats(): Promise<NormalizationStats> {
  return invoke<NormalizationStats>("get_normalization_stats");
}

export async function getAnalysisProgress(): Promise<AnalysisProgress | null> {
  return invoke<AnalysisProgress | null>("get_analysis_progress");
}

export async function analyzeAllSongs(): Promise<void> {
  return invoke("analyze_all_songs");
}

export async function clearNormalizationData(): Promise<void> {
  return invoke("clear_normalization_data");
}

// Playback settings commands
export async function getPlaybackSettings(): Promise<PlaybackSettings> {
  return invoke<PlaybackSettings>("get_playback_settings");
}

export async function setPlaybackSettings(
  settings: PlaybackSettings
): Promise<void> {
  return invoke("set_playback_settings", { settings });
}

// Notification settings commands
export async function getNotificationSettings(): Promise<NotificationSettings> {
  return invoke<NotificationSettings>("get_notification_settings");
}

export async function setNotificationSettings(
  settings: NotificationSettings
): Promise<void> {
  return invoke("set_notification_settings", { settings });
}

export async function sendNowPlayingNotification(
  params: SendNowPlayingNotificationParams
): Promise<boolean> {
  return invoke<boolean>("send_now_playing_notification", {
    title: params.title,
    body: params.body,
    coverArtPath: params.cover_art_path ?? null,
  });
}

// Library sync settings commands
export async function getSyncSettings(): Promise<SyncSettings> {
  return invoke<SyncSettings>("get_sync_settings");
}

export async function setSyncSettings(settings: SyncSettings): Promise<void> {
  return invoke("set_sync_settings", { settings });
}

export async function getSystemTimePreferences(): Promise<SystemTimePreferences> {
  return invoke<SystemTimePreferences>("get_system_time_preferences");
}

// Last.fm commands
export async function getLastfmStatus(): Promise<LastfmStatus> {
  return invoke<LastfmStatus>("get_lastfm_status");
}

export async function beginLastfmAuth(): Promise<LastfmAuthStart> {
  return invoke<LastfmAuthStart>("begin_lastfm_auth");
}

export async function completeLastfmAuth(): Promise<LastfmStatus> {
  return invoke<LastfmStatus>("complete_lastfm_auth");
}

export async function disconnectLastfm(): Promise<LastfmStatus> {
  return invoke<LastfmStatus>("disconnect_lastfm");
}

export async function getLastfmQueue(): Promise<LastfmQueueItem[]> {
  return invoke<LastfmQueueItem[]>("get_lastfm_queue");
}

export async function retryLastfmQueue(): Promise<number> {
  return invoke<number>("retry_lastfm_queue");
}

// Tray commands
export async function setTrayUpdateAvailable(
  version: string | null
): Promise<void> {
  return invoke("set_tray_update_available", { version });
}
