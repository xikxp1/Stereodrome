-- Stereodrome SQLite Schema

CREATE TABLE IF NOT EXISTS artists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    album_count INTEGER DEFAULT 0,
    cover_art_id TEXT,
    synced_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS albums (
    id TEXT PRIMARY KEY,
    artist_id TEXT NOT NULL,
    name TEXT NOT NULL,
    year INTEGER,
    song_count INTEGER DEFAULT 0,
    duration INTEGER,
    cover_art_id TEXT,
    synced_at TEXT NOT NULL,
    FOREIGN KEY (artist_id) REFERENCES artists(id)
);

CREATE TABLE IF NOT EXISTS songs (
    id TEXT PRIMARY KEY,
    album_id TEXT NOT NULL,
    artist_id TEXT NOT NULL,
    title TEXT NOT NULL,
    track_number INTEGER,
    disc_number INTEGER DEFAULT 1,
    duration INTEGER,
    bit_rate INTEGER,
    size INTEGER,
    suffix TEXT,
    content_type TEXT,
    path TEXT,
    year INTEGER,
    genre TEXT,
    synced_at TEXT NOT NULL,
    FOREIGN KEY (album_id) REFERENCES albums(id),
    FOREIGN KEY (artist_id) REFERENCES artists(id)
);

CREATE TABLE IF NOT EXISTS playlists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    song_count INTEGER DEFAULT 0,
    duration INTEGER DEFAULT 0,
    owner TEXT,
    cover_art_id TEXT,
    created_at TEXT NOT NULL,
    changed_at TEXT NOT NULL,
    synced_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS playlist_songs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    playlist_id TEXT NOT NULL,
    song_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    FOREIGN KEY (playlist_id) REFERENCES playlists(id),
    FOREIGN KEY (song_id) REFERENCES songs(id)
);

-- Server connection info
CREATE TABLE IF NOT EXISTS server_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    url TEXT NOT NULL,
    username TEXT NOT NULL,
    last_connected_at TEXT
);

-- Queue persistence
CREATE TABLE IF NOT EXISTS queue_items (
    position INTEGER PRIMARY KEY,
    song_id TEXT NOT NULL,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    album TEXT NOT NULL,
    duration INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS queue_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    current_index INTEGER,
    shuffle INTEGER NOT NULL DEFAULT 0,
    repeat_mode TEXT NOT NULL DEFAULT 'Off'
);

CREATE TABLE IF NOT EXISTS playback_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    current_song_id TEXT,
    position_seconds REAL NOT NULL DEFAULT 0,
    duration_seconds REAL NOT NULL DEFAULT 0,
    was_playing INTEGER NOT NULL DEFAULT 0,
    app_volume REAL NOT NULL DEFAULT 1,
    now_playing_song_id TEXT,
    scrobbled_song_id TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sync_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS download_items (
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    song_id TEXT NOT NULL,
    status TEXT NOT NULL,
    path TEXT,
    bytes INTEGER DEFAULT 0,
    error TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (entity_type, entity_id, song_id)
);

CREATE TABLE IF NOT EXISTS cover_cache (
    cover_art_id TEXT NOT NULL,
    size INTEGER NOT NULL,
    path TEXT NOT NULL,
    bytes INTEGER DEFAULT 0,
    last_accessed_at TEXT NOT NULL,
    PRIMARY KEY (cover_art_id, size)
);

-- Normalization data (EBU R128 loudness analysis results)
CREATE TABLE IF NOT EXISTS normalization_data (
    song_id TEXT PRIMARY KEY,
    track_loudness_lufs REAL NOT NULL,
    track_peak REAL NOT NULL,
    album_id TEXT,
    source TEXT NOT NULL DEFAULT 'ebur128',
    analyzed_at TEXT NOT NULL,
    FOREIGN KEY (song_id) REFERENCES songs(id)
);

CREATE INDEX IF NOT EXISTS idx_normalization_album ON normalization_data(album_id);

-- Indexes for common queries
CREATE INDEX IF NOT EXISTS idx_artists_name ON artists(name);
CREATE INDEX IF NOT EXISTS idx_albums_artist_id ON albums(artist_id);
CREATE INDEX IF NOT EXISTS idx_albums_artist_year_name ON albums(artist_id, year, name);
CREATE INDEX IF NOT EXISTS idx_albums_name ON albums(name);
CREATE INDEX IF NOT EXISTS idx_songs_album_id ON songs(album_id);
CREATE INDEX IF NOT EXISTS idx_songs_album_disc_track ON songs(album_id, disc_number, track_number);
CREATE INDEX IF NOT EXISTS idx_songs_artist_id ON songs(artist_id);
CREATE INDEX IF NOT EXISTS idx_songs_artist_album_disc_track ON songs(artist_id, album_id, disc_number, track_number);
CREATE INDEX IF NOT EXISTS idx_playlist_songs_playlist_id ON playlist_songs(playlist_id, position);
CREATE INDEX IF NOT EXISTS idx_playlist_songs_song_id ON playlist_songs(song_id);
CREATE INDEX IF NOT EXISTS idx_sync_state_updated_at ON sync_state(updated_at);
CREATE INDEX IF NOT EXISTS idx_download_items_song_id ON download_items(song_id);
CREATE INDEX IF NOT EXISTS idx_download_items_status ON download_items(status);
CREATE INDEX IF NOT EXISTS idx_cover_cache_accessed ON cover_cache(last_accessed_at);
