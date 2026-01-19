// Connection types
export interface ConnectionStatus {
  connected: boolean;
  server_url: string | null;
  username: string | null;
  server_version: string | null;
}

export interface ConnectParams {
  url: string;
  username: string;
  password: string;
}

// Library types
export interface Artist {
  id: string;
  name: string;
  album_count: number;
  cover_art_id: string | null;
  synced_at: string;
}

export interface Album {
  id: string;
  artist_id: string;
  name: string;
  year: number | null;
  song_count: number;
  duration: number | null;
  cover_art_id: string | null;
  synced_at: string;
  // Joined fields
  artistName?: string;
}

export interface Song {
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
  // Joined fields
  artist?: string;
  album?: string;
}

export interface Playlist {
  id: string;
  name: string;
  song_count: number;
  duration: number;
  created_at: string;
  changed_at: string;
  synced_at: string;
}

// Playback types
export interface PlaybackState {
  current_song: Song | null;
  is_playing: boolean;
  position: number;
  duration: number;
  volume: number;
}

// Queue types
export interface QueueItem {
  song_id: string;
  title: string;
  artist: string;
  album: string;
  duration: number;
}

export type RepeatMode = "Off" | "All" | "One";

export interface QueueState {
  items: QueueItem[];
  current_index: number | null;
  shuffle: boolean;
  repeat_mode: RepeatMode;
}

// Sync types
export interface SyncResult {
  artists: number;
  albums: number;
  songs: number;
}

// Search types
export interface SearchResult {
  id: string;
  title: string;
  artist: string;
  album: string;
  result_type: "song" | "album" | "artist";
}
