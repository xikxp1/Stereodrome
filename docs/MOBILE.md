# Mobile Client Feature Parity Plan

This document describes a plan to bring the React Native mobile client to feature parity with the mature desktop Svelte/Tauri client without copying the desktop UX directly. The goal is capability parity: the mobile app should provide the same music, playback, sync, cache, server, and settings behavior through mobile-native interaction patterns.

## Current State

### Desktop

The desktop app is a Svelte SPA hosted by Tauri. The frontend calls Tauri commands from `src/lib/api/commands.ts`, while most behavior lives in `src-tauri/src`.

Key desktop backend surfaces:

- Authentication and Subsonic session restore: `src-tauri/src/commands/auth.rs`, `src-tauri/src/client/*`.
- Local SQLite library and migrations: `src-tauri/src/db/schema.sql`, `src-tauri/src/db/mod.rs`.
- Full, incremental, and reconcile library sync with scheduler/status events: `src-tauri/src/commands/library.rs`.
- Local search index with Tantivy: `src-tauri/src/search/mod.rs`.
- Playlist sync and playlist mutation: `src-tauri/src/commands/playlist.rs`.
- Rust playback engine: `src-tauri/src/audio/player.rs`.
- Queue model, persistence, shuffle, repeat, reroll, next/previous semantics: `src-tauri/src/audio/queue.rs`, `src-tauri/src/commands/queue.rs`, `src-tauri/src/db/queue.rs`.
- Audio cache with LRU size limit and prefetch: `src-tauri/src/cache/audio.rs`, `src-tauri/src/commands/cache.rs`.
- Cover art cache: `src-tauri/src/commands/coverart.rs`.
- Loudness analysis, normalization, dynamics, binaural crossfeed, equalizer, spectrum: `src-tauri/src/audio/*`, `src-tauri/src/commands/normalization.rs`, `src-tauri/src/commands/settings.rs`.
- Now playing and scrobble integration: `src-tauri/src/commands/nowplaying.rs`.
- Media keys, desktop notifications, tray, mini/nano player: `src-tauri/src/media/*`, `src-tauri/src/tray/*`, `src-tauri/src/commands/windowing.rs`.

Desktop frontend surfaces include dense library browsing, queue management, settings for server/sync/playback/cache/normalization, playlist editing, context menus, keyboard shortcuts, notifications, and mini-player modes.

### Mobile

The mobile app is an Expo React Native client under `mobile/`.

Implemented mobile surfaces:

- Expo app shell with an iPod-style click wheel UI: `mobile/src/components/IpodShell.tsx`, `mobile/src/components/ClickWheel.tsx`.
- Basic navigation stack: `mobile/src/context/ViewContext.tsx`.
- Local mobile-only setting for handedness: `mobile/src/context/MobileSettingsContext.tsx`.
- Rust FFI bridge: `crates/stereodrome-core`, `crates/stereodrome-ffi`, `mobile/modules/stereodrome-core`.
- Core calls from React Native: `mobile/src/services/stereodromeCore.ts`.
- Basic auth/session restore, manual full library sync, artists/albums/songs, search, playlist reads, signed cover art URLs, signed stream URLs.
- Playback through `react-native-track-player`: `mobile/src/context/PlaybackContext.tsx`, `mobile/src/services/playbackService.ts`.
- Basic now playing screen with current/next song, progress, and cover art URL loading: `mobile/src/screens/NowPlayingScreen.tsx`.

Important mobile limitations:

- Playback does not use the Rust desktop audio backend, so equalizer, normalization, dynamics, binaural crossfeed, Rust-managed gapless/crossfade, spectrum, cache-backed playback, and backend scrobble timing are absent.
- Queue state is JavaScript-only and not persisted through the shared SQLite queue tables.
- Playback position/current item is not persisted.
- Playback state is not reported consistently to the server. The desktop reports "now playing" on track start and submits a scrobble at 50 percent playback.
- Mobile cover art is fetched through signed URLs and platform image caching, not the desktop cover cache.
- Mobile audio streams are signed URLs handed to TrackPlayer, not cached files from the Rust audio cache.
- Mobile sync is a simple full sync in `crates/stereodrome-core/src/lib.rs`; it does not implement desktop incremental sync, reconcile sync, scheduler status, search indexing, or event notifications.
- Mobile playlist support is read-only in the UI and mostly live-server backed in the core.
- Mobile settings do not expose desktop playback, normalization, cache, sync, notification, or server scan settings.

## Parity Principle

Mobile should not mirror the desktop layout. It should preserve the same capabilities with mobile-native UX:

- Use compact screens, bottom sheets, long-press menus, swipe actions, and platform media controls instead of desktop sidebars, context menus, and window chrome.
- Keep the click-wheel mode if it remains a product goal, but do not let it block touch-native workflows for library management, queue editing, search, and settings.
- Treat desktop-only surfaces as platform-adapted features. Examples: the desktop mini-player becomes lock-screen/notification/Live Activity style controls; desktop tray controls become Android notification actions and iOS Control Center integration.

## Target Architecture

### Shared Core

Move parity-critical behavior out of the Tauri adapter and into shared crates so desktop and mobile cannot drift.

Recommended crate split:

- `stereodrome-core`: platform-neutral Subsonic client orchestration, database, sync, search, playlists, queue model, settings models, cache policy, scrobble policy.
- `stereodrome-audio`: reusable decoding/DSP pipeline: loudness analysis, normalization gain calculation, dynamics, binaural crossfeed, equalizer, spectrum analysis, gapless/crossfade planning.
- `stereodrome-ffi`: mobile API boundary. It can keep JSON-over-FFI short term, but should move toward a typed surface, preferably UniFFI, once the API stabilizes.
- `src-tauri`: desktop adapter only. Owns Tauri commands/events, windowing, tray, desktop notifications, media key wiring.
- `mobile/modules/stereodrome-core`: iOS/Android adapter only. Owns background service integration, event emission to JS, and platform permission/media-session plumbing.

The desktop code already has partial sharing via `crates/stereodrome-core`, but that crate currently implements only a subset and duplicates simplified behavior. The next phase should move desktop implementations into shared crates rather than reimplementing mobile features separately.

### Mobile Native Service

Mobile needs a long-lived native playback/core service with event delivery to React Native. React components should be views over backend state, not owners of playback truth.

Required service responsibilities:

- Initialize Rust core once and keep it alive across app foreground/background transitions.
- Own queue state, playback state, cache state, settings, and sync jobs.
- Emit events to JS for playback state, queue changes, sync status, normalization progress, cache updates, errors, and connection changes.
- Support background audio, interruptions, headphone/bluetooth controls, lock-screen metadata, notification controls, and app process restoration.
- Persist state atomically so force-kill/restart does not lose queue, playback position, settings, or sync metadata.

### Playback Engine Decision

The largest open technical decision is how to combine Rust DSP parity with mobile background audio reliability.

Recommended path:

1. Keep `react-native-track-player` only as a transitional playback adapter while backend parity is moved into Rust.
2. Extract desktop playback policy and DSP from `src-tauri/src/audio/*` into a mobile-usable crate.
3. Build or choose a mobile audio output adapter that can feed processed PCM through platform-native audio sessions/services.
4. Keep platform media session control native, even if the audio pipeline is Rust-owned.

Do not try to emulate the desktop DSP with JavaScript TrackPlayer options. That would preserve neither behavior nor testability. If a full Rust audio output path proves too risky at first, ship staged parity:

- Stage A: Rust owns queue, cache, sync, scrobble, settings, and signed/downloaded media files; TrackPlayer plays local cached files.
- Stage B: Rust owns loudness analysis and normalization metadata; TrackPlayer continues playback without DSP parity.
- Stage C: Rust/native audio pipeline applies equalizer, normalization, dynamics, binaural crossfeed, gapless, crossfade, and spectrum.

## Feature Parity Matrix

| Area                         | Desktop status                                                     | Mobile status                         | Mobile parity target                                                                 |
| ---------------------------- | ------------------------------------------------------------------ | ------------------------------------- | ------------------------------------------------------------------------------------ |
| Auth/session                 | Persistent server credentials and restore                          | Basic restore via JSON config         | Use shared credential/session handling with secure storage per platform              |
| Library database             | SQLite schema and migrations                                       | Same schema included, simplified init | Shared migrations and versioned migration runner                                     |
| Manual sync                  | Full sync                                                          | Full sync, simplified                 | Shared full sync implementation                                                      |
| Incremental sync             | Newest-album incremental sync with scheduler                       | Missing                               | Background-capable incremental sync with status and errors                           |
| Reconcile sync               | Full reconcile path                                                | Missing                               | Shared reconcile path with mobile UI controls                                        |
| Sync settings/status         | Configurable intervals, status events                              | Missing                               | Mobile settings and background job visibility                                        |
| Search                       | Tantivy index                                                      | SQL LIKE search                       | Shared indexed search or mobile-compatible equivalent                                |
| Artist/album/song browsing   | Mature desktop views                                               | Basic lists                           | Mobile optimized browse, sort, sections, fast lists, offline reads                   |
| Album lists                  | Recent/played/newest style views via server                        | Basic `getAlbumList`                  | Mobile discovery screens with cached metadata where possible                         |
| Playlists                    | Sync, create, rename, delete, add/remove songs                     | Read-only screens                     | Full playlist management with mobile gestures/forms                                  |
| Playback engine              | Rust/rodio pipeline                                                | TrackPlayer streaming URLs            | Backend-owned playback, cached media, DSP parity                                     |
| Queue                        | Rust queue, persisted, events                                      | JS state only                         | Shared queue engine, persisted, event-driven UI                                      |
| Shuffle/repeat               | Off/All/One, prepared shuffle cycle                                | Boolean repeat and JS shuffle         | Exact shared semantics                                                               |
| Reroll next                  | Backend supported                                                  | JS local swap                         | Shared queue operation                                                               |
| Gapless                      | Rust same-album/consecutive append                                 | Missing                               | Backend-planned gapless where audio adapter supports it                              |
| Crossfade                    | Configurable, manual advance option                                | Missing                               | Configurable crossfade with mobile setting                                           |
| Volume                       | Runtime and persisted volume                                       | Platform player only                  | Persisted app volume where meaningful, respect platform volume rules                 |
| Equalizer                    | 12-band EQ with presets                                            | Missing                               | Mobile EQ UI and backend processing                                                  |
| Normalization                | Track/album LUFS, preamp, clipping prevention                      | Missing                               | Shared analysis/settings/gain application                                            |
| Dynamics                     | Light/medium/heavy compression                                     | Missing                               | Shared dynamics presets and mobile setting                                           |
| Binaural                     | Crossfeed presets                                                  | Missing                               | Shared crossfeed presets and mobile setting                                          |
| Spectrum                     | Backend spectrum event and desktop display                         | Missing                               | Optional mobile visualization using backend event                                    |
| Audio cache                  | LRU file cache, size limits, prefetch                              | Missing                               | Local audio cache, offline-ready playback, cache settings                            |
| Cover cache                  | Local cover cache                                                  | Signed URLs only                      | Local cover cache and prefetch                                                       |
| Playback restore             | Queue persists; volume persists; current audio not restored        | Missing                               | Restore queue, current song, position, repeat/shuffle, play/pause intent             |
| Server now playing           | Reports on track start                                             | Missing/inconsistent                  | Submit `scrobble(..., submission=false)` on starts/resumes                           |
| Scrobble submit              | 50 percent threshold                                               | Missing                               | Shared scrobble threshold policy                                                     |
| Now playing feed             | Polls server every 5 seconds                                       | Missing                               | Mobile now-playing/social screen if product wants parity                             |
| Server scan                  | Status and start scan                                              | Missing                               | Mobile server scan controls                                                          |
| Notifications/media controls | Desktop notifications, media keys, tray                            | TrackPlayer remote handlers           | Android notification actions, iOS Control Center, lock-screen metadata               |
| Settings                     | Cache, scan, sync, display, notifications, playback, normalization | Server sync and handedness only       | Mobile settings grouped by Server, Sync, Playback, Cache, Audio Processing, Controls |
| Updates                      | Desktop updater                                                    | Not applicable                        | Use App Store/TestFlight/Play distribution; no in-app parity required                |
| Mini/nano player             | Desktop windowing feature                                          | Not applicable                        | Replace with mobile lock-screen/notification/compact now-playing affordances         |

## Implementation Roadmap

### Phase 1: Stabilize Shared Core Boundaries

Goal: make mobile consume the same non-audio rules as desktop.

Tasks:

- Move database migration handling from `src-tauri/src/db/mod.rs` into `crates/stereodrome-core`.
- Keep `src-tauri/src/db/schema.sql` as the canonical schema or move it into the core crate and include it from desktop.
- Add a shared settings model for playback, normalization, sync, notification/mobile controls, and cache settings.
- Move `PlayQueue` and queue persistence into the shared crate.
- Add mobile FFI methods for:
  - `getQueue`
  - `playSongWithQueue`
  - `addToQueue`
  - `addSongsToQueue`
  - `insertNext`
  - `removeFromQueue`
  - `clearQueue`
  - `moveQueueItem`
  - `playQueueItem`
  - `playNext`
  - `playPrevious`
  - `toggleShuffle`
  - `setRepeatMode`
  - `cycleRepeatMode`
  - `rerollNext`
- Add event delivery from native modules to React Native for `queue-changed`, `playback-state`, `sync-status-changed`, and error events.
- Replace `PlaybackContext` queue ownership with backend queue state.

Acceptance criteria:

- Mobile queue survives app restart.
- Mobile shuffle/repeat/reroll behavior matches desktop tests.
- Desktop and mobile use the same queue implementation.
- React Native no longer computes next-song semantics independently.

### Phase 2: Sync and Library Parity

Goal: mobile library state and search behave like desktop.

Tasks:

- Move desktop full sync, incremental sync, reconcile sync, sync job locking, status keys, and status calculation into shared core.
- Port or share the desktop search indexing path. If Tantivy is too heavy for mobile initially, define a shared `SearchBackend` trait and provide:
  - Tantivy backend for desktop.
  - SQLite FTS5 or optimized SQL backend for mobile.
- Persist playlists locally on mobile instead of using only live playlist fetches.
- Add shared playlist mutation methods: create, rename, delete, add songs, remove songs.
- Add mobile background sync policy:
  - Foreground sync on app open if due.
  - Background task where platform allows.
  - Clear status when background execution is deferred by the OS.
- Add conflict/error reporting for playlist writes and sync failures.

Acceptance criteria:

- Mobile supports manual incremental sync and full reconcile.
- Mobile settings show last attempt, last success, last error, and next run.
- Mobile search results are fast on large libraries and include songs, albums, artists.
- Playlist edits made on mobile round-trip through the server and refresh local state.

### Phase 3: Cache and Offline Foundation

Goal: mobile playback and browsing should not depend on repeatedly streaming remote URLs.

Tasks:

- Move audio cache policy from `src-tauri/src/cache/audio.rs` into shared core with platform-provided data/cache directories.
- Add mobile cache commands:
  - `getAudioCacheStats`
  - `setMaxCacheSize`
  - `clearAudioCache`
  - `isSongCached`
  - `downloadSong`
  - `removeCachedSong`
  - `downloadAlbum`
  - `downloadPlaylist`
  - `prefetchNext`
- Move cover cache logic into shared core or add a mobile-equivalent cache with the same keying by cover art id and size.
- Change mobile playback from signed stream URLs to local cached files when available.
- Add cache-aware UI:
  - Download/remove actions for song, album, artist, playlist.
  - Cache status indicators.
  - Cache size and clear controls.
  - Optional "Wi-Fi only downloads" setting.
- Add stale signed URL handling. Cached files should not require valid stream URLs.

Acceptance criteria:

- A downloaded album plays with network disabled.
- Cover art for downloaded or recently viewed items loads without network.
- Cache size limit is enforced with predictable LRU behavior.
- Next-track prefetch works from backend queue state.

### Phase 4: Playback State, Server State, and Background Behavior

Goal: backend owns playback truth and server reporting.

Tasks:

- Persist playback state:
  - queue items
  - current index
  - position
  - duration
  - current song metadata
  - shuffle
  - repeat mode
  - volume/app gain setting
  - play/pause state at last background/termination point
- Define resume policy:
  - Default: restore paused at last position after cold start.
  - Do not auto-play after force-kill unless the platform service was already playing.
  - Resume with explicit user action from app, lock screen, headset, or notification.
- Move scrobble policy into shared core:
  - now-playing report on track start
  - submit at 50 percent
  - avoid duplicate submits on seeks/repeats
  - support timestamp where the server accepts it
- Add mobile native media session integration:
  - Android foreground service notification with play/pause/next/previous/seek.
  - iOS Control Center metadata and remote command handling.
  - Interruption handling for calls, route changes, bluetooth/headphones.
  - Lock-screen cover art from local cover cache.
- Add app lifecycle handling:
  - Flush playback state on background.
  - Continue playback in background.
  - Rehydrate state before React renders now-playing screens.

Acceptance criteria:

- Playback continues with screen locked.
- Lock-screen controls operate the backend queue, not a JS-only queue.
- Restarting the app restores queue and position.
- Server now-playing/scrobble behavior matches desktop.

### Phase 5: Audio Processing Parity

Goal: mobile audio output supports the same processing capabilities as desktop.

Tasks:

- Extract reusable DSP modules:
  - `loudness.rs`
  - `normalizer.rs`
  - `dynamics.rs`
  - `compressor.rs`
  - `binaural.rs`
  - `equalizer.rs`
  - `spectrum.rs`
- Define a common playback graph:
  - source decode
  - seek
  - optional normalizer
  - optional dynamics
  - optional binaural crossfeed
  - optional equalizer
  - spectrum tap
  - output adapter
- Add mobile-compatible decode/output strategy. Validate platform constraints before committing:
  - Android: native service plus audio output adapter that can run while backgrounded.
  - iOS: AVAudioSession-compatible output path with interruption and route handling.
- Implement loudness analysis on mobile using cached audio files.
- Add normalization settings UI:
  - enable/disable
  - track/album mode
  - target LUFS presets
  - preamp
  - prevent clipping
  - analyze all
  - clear analysis
  - analysis progress
- Add playback settings UI:
  - gapless
  - crossfade
  - manual advance crossfade
  - crossfade duration
  - binaural enable/preset
  - EQ enable/presets/12-band editor
  - dynamics enable/preset
- Add a spectrum event and optional mobile visualization if it fits the mobile UI.

Acceptance criteria:

- Audio processing settings can be changed while a song is playing and are reapplied consistently.
- Normalization gain calculation matches desktop for the same database rows.
- Equalizer presets and custom band clamping match desktop.
- Gapless and crossfade behavior match desktop semantics where platform audio permits it.

### Phase 6: Mobile UX Completion

Goal: expose parity features in a mobile-native way.

Screens and flows:

- Home:
  - Now Playing
  - Library
  - Search
  - Downloads/Offline
  - Settings
- Library:
  - Artists
  - Albums
  - Songs
  - Recently Added
  - Recently Played if server data is available
  - Most Played if server data is available
  - Playlists
- Artist:
  - Albums grouped by year where useful.
  - Play artist, shuffle artist, download artist.
- Album:
  - Cover art, metadata, track list by disc/track.
  - Play, shuffle, download/remove download.
- Song list:
  - Tap to play from context queue.
  - Long press for Play Next, Add to Queue, Add to Playlist, Go to Artist, Go to Album, Download.
  - Multi-select can be a later enhancement, but batch actions should exist for albums/playlists.
- Queue:
  - Current queue list.
  - Reorder by drag handle.
  - Swipe/remove.
  - Clear queue.
  - Shuffle/repeat/reroll.
  - Scroll to current.
- Now Playing:
  - Cover art, title, artist, album.
  - Seek bar.
  - Play/pause, next, previous, reroll.
  - Shuffle/repeat.
  - Queue button.
  - Audio processing quick access.
  - Next track preview.
- Playlists:
  - Create, rename, delete.
  - Add/remove songs.
  - Download playlist.
- Downloads:
  - Downloaded albums/playlists/songs.
  - In-progress downloads.
  - Failed downloads with retry.
  - Cache usage.
- Settings:
  - Server: connection, disconnect, server version, scan status/start scan.
  - Library Sync: incremental/reconcile settings and status.
  - Playback: gapless, crossfade, queue behavior.
  - Audio Processing: normalization, dynamics, binaural, equalizer.
  - Cache: size, clear, Wi-Fi only, downloaded content management.
  - Controls: handedness, haptics, click-wheel mode, touch controls.
  - About: app version and diagnostics.

Interaction guidance:

- Use long press and action sheets instead of desktop right-click context menus.
- Keep destructive operations behind confirmation dialogs.
- Use platform list virtualization for large libraries.
- Keep offline/download state visible but unobtrusive.
- Keep the click wheel as an alternate control layer; touch gestures should still work.
- Avoid desktop-specific controls such as window positioning, tray behavior, mini-player position, and updater settings.

## FFI/API Plan

The current JSON-over-FFI dispatch in `crates/stereodrome-ffi/src/lib.rs` is useful for iteration but will become hard to maintain as evented playback and background operations grow.

Short-term requirements:

- Keep JSON envelopes but add typed TypeScript wrappers for every method.
- Standardize method names with desktop command names where practical.
- Add structured error codes, not only strings.
- Add event subscription from native to JS.
- Avoid creating a new Tokio runtime per call for hot paths. The current dispatch creates a runtime inside `dispatch`; playback, sync, and cache work need a long-lived runtime.

Medium-term requirements:

- Move to UniFFI or another typed bridge so Swift/Kotlin/TypeScript bindings stay in sync.
- Model long-running operations as jobs with ids, progress events, cancellation, and final results.
- Separate platform adapter errors from core errors.

## Data and Migration Plan

Shared database rules:

- `src-tauri/src/db/schema.sql` is currently reused by `crates/stereodrome-core/src/db.rs`. Keep one canonical schema.
- Move migration version tracking into shared core so desktop and mobile apply the same migrations.
- Any schema additions for playback state, downloads, cache metadata, or settings must include migration steps for existing desktop and mobile databases.

Likely schema additions:

- `playback_state`: current song, position, duration, updated_at, was_playing.
- `download_items`: entity type, entity id, song id, status, bytes, error, updated_at.
- `cover_cache`: cover art id, size, path, bytes, last_accessed_at.
- Optional cache metadata table if filesystem metadata is not reliable on mobile.
- Settings table or JSON store replacement if a shared settings mechanism is preferred over platform-specific stores.

Credential handling:

- Desktop currently uses its credential/session path.
- Mobile currently writes `server_config.json` with password in the app document directory.
- Move mobile credentials to Keychain on iOS and Android Keystore/EncryptedSharedPreferences on Android.
- Keep non-secret server metadata in SQLite or a config file.

## Validation Plan

Backend/shared tests:

- Queue behavior tests for all repeat/shuffle/reroll/pending-navigation cases.
- Sync unit tests for status calculation, newest-head incremental behavior, reconcile deletes/updates, and failed job status.
- Migration tests from old schemas to current schema.
- Cache tests for filename sanitization, LRU eviction, size limits, and clear operations.
- Normalization gain tests for track mode, album mode, preamp, and clipping prevention.
- Scrobble policy tests for threshold, repeat, seek backward, and duplicate prevention.

Mobile integration tests:

- Native module initializes on iOS and Android.
- Session restore works after app restart.
- Queue persists after app restart.
- Playback continues in background and lock-screen controls work.
- Offline album playback after download.
- Search performance with a large local library fixture.
- Playlist create/rename/delete/add/remove round-trips through a test Subsonic server.

Manual QA:

- Android: notification controls, bluetooth controls, foreground service survival, app swipe-away behavior, network loss.
- iOS: Control Center, lock-screen metadata, route changes, interruptions, background audio, TestFlight install.
- Both: low-storage cache behavior, large library sync, long album gapless playback, crossfade transitions, EQ/normalization toggles while playing.

## Suggested Milestones

1. Shared queue and playback state persistence.
2. Mobile event bridge and backend-owned queue UI.
3. Shared sync/reconcile/status and mobile sync settings.
4. Audio and cover cache with local-file playback.
5. Server now-playing/scrobble parity.
6. Playlist mutation parity.
7. Audio processing extraction and mobile output proof of concept.
8. Normalization/EQ/dynamics/binaural settings UI.
9. Offline/downloads UX.
10. Mobile polish, accessibility, and platform media controls.

## Immediate Next Work

The best first implementation slice is shared queue ownership on mobile:

- It touches important architecture without requiring the hardest mobile audio-output decision.
- It removes the largest current state drift between desktop and mobile.
- It enables persistence, scrobbling, prefetch, downloads, and background controls to build on one source of truth.
- It can reuse existing desktop queue tests and behavior almost directly.

After that, implement audio/cover cache and local-file playback. Once TrackPlayer can play cached files from backend queue state, the app has a stable foundation for offline support and later DSP work.
