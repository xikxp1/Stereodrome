import { invoke } from "@tauri-apps/api/core";
import type {
  ConnectionStatus,
  ConnectParams,
  Artist,
  Album,
  Song,
  SyncResult,
  ScanStatus,
  PlaybackState,
  SearchResults,
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

export async function getArtists(): Promise<Artist[]> {
  return invoke<Artist[]>("get_artists");
}

export async function getAlbums(artistId?: string): Promise<Album[]> {
  return invoke<Album[]>("get_albums", { artistId });
}

export async function getSongs(
  albumId?: string,
  artistId?: string
): Promise<Song[]> {
  return invoke<Song[]>("get_songs", { albumId, artistId });
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

export async function seekPlayback(position: number): Promise<void> {
  return invoke("seek_playback", { position });
}

export async function getPlaybackStatus(): Promise<PlaybackState> {
  return invoke<PlaybackState>("get_playback_status");
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

// Tray commands
export async function setTrayUpdateAvailable(
  version: string | null
): Promise<void> {
  return invoke("set_tray_update_available", { version });
}
