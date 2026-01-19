# Stereodrome Development Plan

Last updated: 2026-01-19

## Current Status

**Phase:** Initial Setup | **Version:** 0.1.0

Basic Tauri + Svelte 5 boilerplate in place. No music player functionality yet.

## Immediate Priorities

None currently - awaiting feature development tasks.

## Completed

- [x] Project scaffolding (Tauri + Svelte 5)
- [x] Bun package manager setup
- [x] TypeScript strict mode
- [x] Documentation (README.md, CLAUDE.md)

## Planned Features

### Core Player
- [ ] Subsonic server connection and authentication
- [ ] Library browsing (artists, albums, songs)
- [ ] Playback controls (play, pause, skip, volume)
- [ ] Queue management
- [ ] Playlist support
- [ ] Search functionality

### Local Storage
- [ ] SQLite schema for metadata
- [ ] Tantivy index for search
- [ ] Metadata sync from server
- [ ] Incremental sync

### UI
- [ ] iTunes-inspired layout
- [ ] Sidebar navigation
- [ ] Album/artist views
- [ ] Now playing interface
- [ ] Queue panel

## Known Issues

None.

## Next Steps

Suggested starting points:

1. **Subsonic API** - Rust client for server communication
2. **SQLite Schema** - Database design for metadata
3. **Basic UI Layout** - Main interface structure
4. **Auth Flow** - Server connection and login

---

Update this file after completing work.
