# Stereodrome Development Plan

Last updated: 2026-01-20

## Current Status

**Phase:** Phase 6 Complete | **Version:** 0.1.0

Core player functionality implemented: audio playback with Rodio, queue management, playlist support, and search functionality. Users can play music, manage queues, create playlists, and search their library.

## Immediate Priorities

Phase 7: UI Polish & Optimization

- [x] TanStack Virtual for large song lists
- [x] Now playing from local backend (combined playback-state event, no server latency)
- [x] Refactored components to use DaisyUI (menu, card, input, btn, badge, alert)
- Queue panel
- Keyboard shortcuts

## Completed

- [x] Project scaffolding (Tauri + Svelte 5)
- [x] Bun package manager setup
- [x] TypeScript strict mode
- [x] Documentation (README.md, CLAUDE.md)

### Phase 1: Foundation & Authentication

- [x] Rust error handling (thiserror)
- [x] App state management (Mutex-wrapped client, db)
- [x] SQLite database schema and init
- [x] Database migrations for schema changes
- [x] Submarine crate integration for Subsonic API
- [x] Auth commands (connect_server, disconnect, get_status, restore_session)
- [x] Persistent credentials with tauri-plugin-store
- [x] Tailwind CSS + DaisyUI setup
- [x] TanStack Query client
- [x] Connection store (Svelte 5 runes)
- [x] ServerConnect component
- [x] TypeScript types for all entities
- [x] Main layout with Query provider

### Phase 2: Library Sync & Browsing

- [x] Library commands (sync_library, get_artists, get_albums, get_songs)
- [x] TanStack Query hooks for library data
- [x] Sidebar with sync button
- [x] ArtistList component
- [x] AlbumGrid component
- [x] SongList component
- [x] Main library browsing UI

### Phase 3: Audio Playback

- [x] AudioPlayer module with Rodio integration
- [x] Stream module for fetching audio from Subsonic
- [x] Playback commands (play_song, pause, resume, stop, set_volume)
- [x] Playback store with Svelte 5 runes
- [x] Position updates via Tauri events (10Hz)
- [x] TransportBar wired to playback controls

### Phase 4: Queue Management

- [x] PlayQueue struct with shuffle/repeat modes
- [x] Queue commands (add, remove, reorder, play_next, play_previous)
- [x] Queue store with auto-advance on song end
- [x] Play song with queue context (filtered songs)

### Phase 5: Playlist Support

- [x] Playlist commands (create, update, delete, add/remove songs)
- [x] Playlist store with CRUD operations
- [x] Sidebar playlists section with create/select

### Phase 6: Search

- [x] SQL LIKE-based search command
- [x] Search store with debounced queries
- [x] TransportBar search with results dropdown

## Planned Features

### Core Player

- [x] Subsonic server connection and authentication
- [x] Library browsing (artists, albums, songs)
- [x] Playback controls (play, pause, skip, volume)
- [x] Queue management
- [x] Playlist support
- [x] Search functionality

### Local Storage

- [x] SQLite schema for metadata
- [x] Tantivy index for full-text search
- [x] Metadata sync from server
- [ ] Incremental sync

### UI

- [x] iTunes-inspired layout with classic aesthetic
- [x] TransportBar with playback controls, LCD display, search
- [x] Sidebar with section headers and icons
- [x] ColumnBrowser (Genres/Artists/Albums)
- [x] SongList with alternating rows, checkboxes, columns
- [x] StatusBar with item count, duration, size
- [x] Custom DaisyUI "itunes" theme
- [x] Now playing in TransportBar (local state via combined playback-state event)
- [x] Scrobbling to Subsonic server on play
- [ ] Queue panel
- [x] TanStack Virtual for large lists

## Architecture

### Backend (Rust)

```
src-tauri/src/
├── lib.rs              # Tauri setup, state, command registration
├── error.rs            # AppError enum with thiserror
├── state.rs            # AppState (client, db, audio_player, queue, search_index)
├── audio/
│   ├── mod.rs          # Module exports
│   ├── player.rs       # AudioPlayer with Rodio (threaded)
│   ├── queue.rs        # PlayQueue with shuffle/repeat
│   └── stream.rs       # Subsonic audio stream fetching
├── commands/
│   ├── mod.rs
│   ├── auth.rs         # connect_server, disconnect, get_status
│   ├── library.rs      # sync_library, get_artists, get_albums, get_songs
│   ├── playback.rs     # play_song, pause, resume, stop, set_volume
│   ├── queue.rs        # Queue management commands
│   ├── playlist.rs     # Playlist CRUD commands
│   ├── search.rs       # Tantivy full-text search
│   └── nowplaying.rs   # Scrobbling, now playing emitter (events)
├── search/
│   └── mod.rs          # IndexManager, SearchSchema, Tantivy integration
└── db/
    ├── mod.rs          # init_db, get_db_path
    └── schema.sql      # SQLite tables
```

### Frontend (Svelte 5)

```
src/lib/
├── api/commands.ts     # Typed invoke wrappers
├── db/
│   ├── queryClient.ts  # TanStack Query client
│   └── collections.ts  # Query factories
├── stores/
│   ├── connection.svelte.ts  # Connection state (runes)
│   ├── playback.svelte.ts    # Playback state with combined event (position + song info)
│   ├── queue.svelte.ts       # Queue management store
│   ├── playlist.svelte.ts    # Playlist store
│   ├── search.svelte.ts      # Search with debounce
│   └── nowplaying.svelte.ts  # Server now playing state (for other users)
├── components/
│   ├── ServerConnect.svelte   # Login screen
│   ├── TransportBar.svelte    # Top toolbar with playback controls + now playing (local state)
│   ├── Sidebar.svelte         # Navigation + playlists
│   ├── StatusBar.svelte       # Bottom status bar
│   └── library/
│       ├── ArtistList.svelte
│       ├── AlbumGrid.svelte
│       ├── ColumnBrowser.svelte  # Genre/Artist/Album browser
│       └── SongList.svelte       # iTunes-style song table
└── types/index.ts      # All TypeScript interfaces
```

## Known Issues

- Dead code warnings for unused error variants (expected)

## Next Steps

1. **Phase 7** - Queue panel UI (now playing panel complete)
2. **Phase 8** - Keyboard shortcuts
3. **Phase 9** - UI polish

---

Update this file after completing work.
