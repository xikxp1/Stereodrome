export type ConnectionStatus = {
  connected: boolean;
  server_url: string | null;
  username: string | null;
  server_version: string | null;
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

export type SyncResult = {
  artists: number;
  albums: number;
  songs: number;
};
