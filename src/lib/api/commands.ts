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
