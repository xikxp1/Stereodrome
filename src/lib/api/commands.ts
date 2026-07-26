import { invoke } from "@tauri-apps/api/core";

import { dispatch } from "$lib/api/core";
import type {
  ConnectionStatus,
  ConnectParams,
  Artist,
  Album,
  AlbumListEntry,
  Song,
  SyncResult,
  ScanStatus,
  SearchResults,
  NormalizationSettings,
  NormalizationStats,
  AnalysisProgress,
  PlaybackSettings,
  NotificationSettings,
  ConnectivitySettings,
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
  CacheLocationInfo,
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
  return dispatch({ type: "get-connection-status" });
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
  return dispatch({ type: "get-library-sync-status" });
}

export async function getArtists(): Promise<Artist[]> {
  return dispatch({ type: "get-artists" });
}

export async function getAlbumCount(): Promise<number> {
  return invoke<number>("get_album_count");
}

export async function getAlbums(artistId?: string): Promise<Album[]> {
  return dispatch({ type: "get-albums", artist_id: artistId ?? null });
}

export async function getAlbumList(
  listType: string,
  size?: number,
  offset?: number
): Promise<AlbumListEntry[]> {
  return dispatch({
    type: "get-album-list",
    list_type: listType,
    size: size ?? null,
    offset: offset ?? null,
  });
}

export async function getSongs(
  albumId?: string,
  artistId?: string
): Promise<Song[]> {
  return dispatch({
    type: "get-songs",
    album_id: albumId ?? null,
    artist_id: artistId ?? null,
  });
}

export async function removeSongsFromPlaylist(
  playlistId: string,
  positions: number[]
): Promise<void> {
  await dispatch({
    type: "remove-songs-from-playlist",
    playlist_id: playlistId,
    song_indexes: positions,
  });
}

export async function setPlaylistSavedOffline(
  playlistId: string,
  savedOffline: boolean
): Promise<SavedPlaylistOfflineResult> {
  return dispatch({
    type: "set-playlist-saved-offline",
    playlist_id: playlistId,
    saved_offline: savedOffline,
  });
}

export async function reconcileSavedPlaylistsOffline(): Promise<
  SavedPlaylistOfflineResult[]
> {
  return dispatch({ type: "reconcile-saved-playlists-offline" });
}

// Playback commands
export async function playSong(songId: string): Promise<void> {
  return dispatch({
    type: "play-selection",
    song_id: songId,
    song_ids: [songId],
  });
}

export async function setVolume(volume: number): Promise<void> {
  return dispatch({ type: "set-playback-volume", volume });
}

export async function seekPlayback(position: number): Promise<void> {
  return dispatch({ type: "seek-to", seconds: position });
}

export interface BackupSummary {
  artists: number;
  albums: number;
  songs: number;
  playlists: number;
  queue_items: number;
}

export async function exportPortableBackup(
  path: string
): Promise<BackupSummary> {
  return dispatch({ type: "export-portable-backup", path });
}

export async function importPortableBackup(
  path: string
): Promise<BackupSummary> {
  return invoke<BackupSummary>("import_portable_backup", { path });
}

// Queue commands
export async function playSongWithQueue(
  songId: string,
  songIds: string[]
): Promise<void> {
  return dispatch({
    type: "play-selection",
    song_id: songId,
    song_ids: songIds,
  });
}

export async function rerollNextQueueItem(): Promise<void> {
  await dispatch({ type: "reroll-next" });
}

// Search commands
export async function searchLibrary(
  query: string,
  limit?: number
): Promise<SearchResults> {
  return dispatch({ type: "search-library", query, limit: limit ?? null });
}

// Cover art commands
export async function getCoverArt(
  coverArtId: string,
  size?: number
): Promise<string> {
  return invoke<string>("get_cover_art", { coverArtId, size });
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

export async function getCacheLocations(): Promise<CacheLocationInfo> {
  return invoke<CacheLocationInfo>("get_cache_locations");
}

export async function getAudioCacheStats(): Promise<CacheStats> {
  return dispatch({ type: "get-audio-cache-stats" });
}

export async function getOfflineSongIds(): Promise<string[]> {
  return dispatch({ type: "get-offline-song-ids" });
}

export async function getDownloadingSongIds(): Promise<string[]> {
  return invoke<string[]>("get_downloading_song_ids");
}

export async function clearAudioCache(): Promise<void> {
  await dispatch({ type: "clear-audio-cache" });
}

export async function setMaxCacheSize(size: number): Promise<CacheStats> {
  return dispatch({ type: "set-max-cache-size", max_size: size });
}

// Scan commands
export async function getScanStatus(): Promise<ScanStatus> {
  return dispatch({ type: "get-scan-status" });
}

export async function startScan(): Promise<ScanStatus> {
  return dispatch({ type: "start-scan" });
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

// Connectivity settings commands
export async function getConnectivitySettings(): Promise<ConnectivitySettings> {
  return invoke<ConnectivitySettings>("get_connectivity_settings");
}

export async function setConnectivitySettings(
  settings: ConnectivitySettings
): Promise<ConnectivitySettings> {
  return invoke<ConnectivitySettings>("set_connectivity_settings", {
    settings,
  });
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
  return dispatch({ type: "get-lastfm-status" });
}

export async function beginLastfmAuth(): Promise<LastfmAuthStart> {
  return dispatch({ type: "begin-lastfm-auth" });
}

export async function completeLastfmAuth(): Promise<LastfmStatus> {
  return dispatch({ type: "complete-lastfm-auth" });
}

export async function disconnectLastfm(): Promise<LastfmStatus> {
  return dispatch({ type: "disconnect-lastfm" });
}

export async function getLastfmQueue(): Promise<LastfmQueueItem[]> {
  return dispatch({ type: "get-lastfm-queue" });
}

export async function retryLastfmQueue(): Promise<number> {
  return dispatch({ type: "retry-lastfm-queue" });
}

// Tray commands
export async function setTrayUpdateAvailable(
  version: string | null
): Promise<void> {
  return invoke("set_tray_update_available", { version });
}
