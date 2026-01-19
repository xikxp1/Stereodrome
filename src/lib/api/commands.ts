import { invoke } from '@tauri-apps/api/core';
import type { ConnectionStatus, ConnectParams, Artist, Album, Song, SyncResult } from '$lib/types';

// Auth commands
export async function connectServer(params: ConnectParams): Promise<ConnectionStatus> {
  return invoke<ConnectionStatus>('connect_server', { params });
}

export async function disconnectServer(): Promise<void> {
  return invoke('disconnect_server');
}

export async function getConnectionStatus(): Promise<ConnectionStatus> {
  return invoke<ConnectionStatus>('get_connection_status');
}

// Library commands
export async function syncLibrary(): Promise<SyncResult> {
  return invoke<SyncResult>('sync_library');
}

export async function getArtists(): Promise<Artist[]> {
  return invoke<Artist[]>('get_artists');
}

export async function getAlbums(artistId?: string): Promise<Album[]> {
  return invoke<Album[]>('get_albums', { artistId });
}

export async function getSongs(albumId?: string): Promise<Song[]> {
  return invoke<Song[]>('get_songs', { albumId });
}
