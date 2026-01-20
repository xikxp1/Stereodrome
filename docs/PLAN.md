# Stereodrome Development Plan

Last updated: 2026-01-20

## Current Status

**Phase:** Phase 8 In Progress | **Version:** 0.1.0

Core player functionality with UI polish: audio playback, queue management, playlist support, search, cover art display, and keyboard shortcuts. Phase 7 complete, now adding Phase 8 features.

## Immediate Priorities

Phase 8: Local Storage & Offline Features

- [x] Local audio cache with LRU eviction (5GB max, automatic cleanup)
- [ ] Cache settings UI (view stats, clear cache)
- [ ] Incremental library sync
- [ ] Crossfade between tracks
- [ ] Gapless playback

## Completed

### Phase 7: UI Polish & Optimization

- [x] TanStack Virtual for large song lists
- [x] Now playing from local backend (combined playback-state event, no server latency)
- [x] Refactored components to use DaisyUI (menu, card, input, btn, badge, alert)
- [x] Queue panel with toggle button in TransportBar
- [x] Single-click queue item navigates to song in SongList (if visible in current filter)
- [x] "Scroll to current" button in QueuePanel header
- [x] Cover art display in TransportBar (thumbnail with click-to-view full size)
- [x] Cover art caching to local filesystem
- [x] Keyboard shortcuts (Space play/pause, ↑/↓ navigate songs, Enter play, Shift+←/→ seek, Mod+↑/↓ volume, Mod+←/→ prev/next, M mute, S shuffle, R repeat, Q queue, V visualizer, / search)
- [x] Audio spectrum visualizer (real-time FFT in Rust, 8 frequency bands, 30Hz updates)

## Completed

### Project Setup

- [x] Project scaffolding (Tauri + Svelte 5)
- [x] Bun package manager setup
- [x] TypeScript strict mode
- [x] Documentation (README.md, CLAUDE.md)

### Phase 1: Foundation & Authentication

- [x] Rust error handling (thiserror)
- [x] Mutex poisoning recovery (MutexExt trait with lock_recover)
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
- [x] Consolidated playback state (volume included in playback-state event)
- [x] TransportBar wired to playback controls

### Phase 4: Queue Management

- [x] PlayQueue struct with shuffle/repeat modes
- [x] Queue commands (add, remove, reorder, play_next, play_previous)
- [x] Queue store with auto-advance on song end
- [x] Play song with queue context (filtered songs)
- [x] Queue persistence to SQLite (survives app restart)
- [x] Queue-changed events (frontend reflects backend state)
- [x] Fix: Next button now advances to next song (was restarting current song when RepeatMode::One was set)
- [x] Fix: Play button now plays current queue item when app opens with persisted queue

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
- [x] Local audio cache with LRU eviction (5GB max)
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
- [x] Queue panel
- [x] TanStack Virtual for large lists
- [x] Cover art display with caching and full-size viewer window
- [x] Standard base64 encoding (replaced custom implementation)
- [x] Audio spectrum visualizer in TransportBar (V to toggle)

## Architecture

### Backend (Rust)

```
src-tauri/src/
├── lib.rs              # Tauri setup, state, command registration
├── error.rs            # AppError enum with thiserror, MutexExt trait
├── state.rs            # AppState (client, db, audio_player, queue, search_index)
├── audio/
│   ├── mod.rs          # Module exports
│   ├── analyzer.rs     # AnalyzingSource wrapper for sample capture
│   ├── player.rs       # AudioPlayer with Rodio (threaded)
│   ├── queue.rs        # PlayQueue with shuffle/repeat
│   ├── spectrum.rs     # FFT analysis, SpectrumAnalyzer, band aggregation
│   └── stream.rs       # Subsonic audio stream fetching
├── cache/
│   ├── mod.rs          # Module exports
│   └── audio.rs        # AudioCache with LRU eviction (5GB max)
├── commands/
│   ├── mod.rs
│   ├── auth.rs         # connect_server, disconnect, get_status
│   ├── cache.rs        # get_audio_cache_stats, clear_audio_cache
│   ├── library.rs      # sync_library, get_artists, get_albums, get_songs
│   ├── playback.rs     # play_song, pause, resume, stop, set_volume
│   ├── queue.rs        # Queue management commands
│   ├── playlist.rs     # Playlist CRUD commands
│   ├── search.rs       # Tantivy full-text search
│   ├── nowplaying.rs   # Scrobbling, now playing emitter (events)
│   └── coverart.rs     # Cover art fetching with local cache
├── search/
│   └── mod.rs          # IndexManager, SearchSchema, Tantivy integration
└── db/
    ├── mod.rs          # init_db, get_db_path
    ├── queue.rs        # Queue persistence (save/load)
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
│   ├── spectrum.svelte.ts    # Spectrum visualizer state (30Hz band updates)
│   └── nowplaying.svelte.ts  # Server now playing state (for other users)
├── components/
│   ├── ServerConnect.svelte   # Login screen
│   ├── TransportBar.svelte    # Top toolbar with playback controls + now playing (local state)
│   ├── SpectrumBars.svelte    # Audio spectrum visualizer (8 bars)
│   ├── Sidebar.svelte         # Navigation + playlists
│   ├── StatusBar.svelte       # Bottom status bar
│   ├── QueuePanel.svelte      # Queue panel with shuffle/repeat controls
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

1. **Phase 8 In Progress** - Local audio cache implemented, continue with settings UI and incremental sync
2. **Phase 9** - Audio enhancements (crossfade, gapless playback)

---

Update this file after completing work.
