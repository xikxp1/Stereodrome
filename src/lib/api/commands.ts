import { invoke } from "@tauri-apps/api/core";
import type {
  ConnectionStatus,
  ConnectParams,
  Artist,
  Album,
  Song,
  SyncResult,
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

export async function getSongs(albumId?: string): Promise<Song[]> {
  return invoke<Song[]>("get_songs", { albumId });
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

export async function getAudioCacheStats(): Promise<CacheStats> {
  return invoke<CacheStats>("get_audio_cache_stats");
}

export async function clearAudioCache(): Promise<void> {
  return invoke("clear_audio_cache");
}

// Scrobbling commands
export async function scrobbleSubmit(songId: string): Promise<void> {
  return invoke("scrobble_submit", { songId });
}
