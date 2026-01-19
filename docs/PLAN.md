# Stereodrome Development Plan

Last updated: 2026-01-19

## Current Status

**Phase:** Phase 2 Complete | **Version:** 0.1.0

Library sync and browsing implemented. Users can connect to a Subsonic server, sync their library, and browse artists/albums/songs.

## Immediate Priorities

Phase 3: Audio Playback
- Implement audio streaming with Rodio
- Create playback controls
- Add now playing UI

## Completed

- [x] Project scaffolding (Tauri + Svelte 5)
- [x] Bun package manager setup
- [x] TypeScript strict mode
- [x] Documentation (README.md, CLAUDE.md)

### Phase 1: Foundation & Authentication
- [x] Rust error handling (thiserror)
- [x] App state management (Mutex-wrapped client, db)
- [x] SQLite database schema and init
- [x] Submarine crate integration for Subsonic API
- [x] Auth commands (connect_server, disconnect, get_status)
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

## Planned Features

### Core Player
- [x] Subsonic server connection and authentication
- [x] Library browsing (artists, albums, songs)
- [ ] Playback controls (play, pause, skip, volume)
- [ ] Queue management
- [ ] Playlist support
- [ ] Search functionality

### Local Storage
- [x] SQLite schema for metadata
- [ ] Tantivy index for search
- [x] Metadata sync from server
- [ ] Incremental sync

### UI
- [x] iTunes-inspired layout (basic)
- [x] Sidebar navigation
- [x] Album/artist views
- [ ] Now playing interface
- [ ] Queue panel

## Architecture

### Backend (Rust)
```
src-tauri/src/
├── lib.rs              # Tauri setup, state, command registration
├── error.rs            # AppError enum with thiserror
├── state.rs            # AppState (client, db, server config)
├── commands/
│   ├── mod.rs
│   ├── auth.rs         # connect_server, disconnect, get_status
│   └── library.rs      # sync_library, get_artists, get_albums, get_songs
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
├── stores/connection.svelte.ts  # Connection state (runes)
├── components/
│   ├── ServerConnect.svelte
│   ├── Sidebar.svelte
│   └── library/
│       ├── ArtistList.svelte
│       ├── AlbumGrid.svelte
│       └── SongList.svelte
└── types/index.ts      # All TypeScript interfaces
```

## Known Issues

- Dead code warnings for unused error variants (expected, will be used in later phases)

## Next Steps

1. **Phase 3** - Audio playback with Rodio
2. **Phase 4** - Queue management
3. **Phase 5** - TanStack Virtual for song lists
4. **Phase 6** - Tantivy search
5. **Phase 7** - UI polish

---

Update this file after completing work.
