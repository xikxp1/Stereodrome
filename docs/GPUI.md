# GPUI Desktop Migration Plan

## Decision summary

- Preserve desktop feature parity on macOS, Windows, and Linux. No supported desktop OS is dropped.
- Preserve the existing application identity, profile, database, search index, caches, settings, state, and keyring entries. Existing users must not see a second profile or reauthenticate because the UI toolkit changed.
- Replace the Tauri webview, IPC commands, plugins, and string-named events with a native GPUI shell, direct Rust calls, and typed Rust events.
- Keep `stereodrome-audio` and `stereodrome-core::queue` as the shared audio and queue engines. Move the existing desktop backend intact instead of rewriting playback, queue, search, cache, sync, keyring, or Last.fm behavior.
- Leave `mobile`, `crates/stereodrome-ffi`, and the mobile-oriented `StereodromeCore` API unchanged. This is not a wholesale switch of the desktop to `StereodromeCore`; only the canonical SQL schema becomes shared.
- Ship GPUI beside the production Tauri app until native builds, copied-profile compatibility, behavior parity, packaging, and signed updates pass on all three desktop OSes. Cut over only after every phase gate in this document is satisfied.

## Scope boundary

This document is the execution contract for the migration. The migration changes the desktop shell and relocates desktop Rust code; it does not redesign the product, database, audio algorithms, Subsonic behavior, or mobile API. Each phase must preserve a shipping Tauri path until its deletion gate explicitly permits removal.

## Final architecture

The final desktop has three layers. Calls only point downward; events only report state upward.

```text
GPUI shell (`ui`, native services, windows, actions)
        │ direct methods / typed results
        ▼
Desktop backend (`DesktopBackend`, `Arc<DesktopState>`, `DesktopEvents`)
        │ concrete Rust calls
        ▼
Existing shared engines (`stereodrome-audio`, `stereodrome-core::queue`)
```

1. **Shared engines.** Keep DSP, decoding, spectrum, and normalization in `crates/stereodrome-audio`. Keep queue semantics and persisted queue types in `stereodrome-core::queue`. Move the unchanged canonical desktop SQL schema into `stereodrome-core`, but do not route the desktop through the mobile-oriented `StereodromeCore`.
2. **Desktop backend library.** Add one workspace package, `crates/stereodrome-desktop`, with package name `stereodrome-desktop` and library name `stereodrome_desktop`. Its concrete `DesktopBackend` owns an `Arc<DesktopState>`. `DesktopState` owns the desktop Subsonic client, SQLite connection, audio player, queue, Tantivy index, settings/state JSON stores, cache locations, Last.fm tracker, Tokio runtime, cancellation tokens, retained worker handles, and typed events. Preserve the existing concrete ownership graph from `src-tauri/src/state.rs::AppState`; do not add provider, repository, service, or adapter trait families.
3. **GPUI shell.** The same package exposes a binary named `stereodrome`. The binary owns GPUI entities, windows, menus, prompts, updater presentation, tray, notifications, media controls, single-instance IPC, and platform window flags. It calls `DesktopBackend` directly and projects backend snapshots/events into one shared GPUI application model.

### Package and feature boundary

During coexistence:

- Add `crates/stereodrome-desktop` to the root workspace.
- Define `default = []` and a `gpui-ui` feature containing GPUI, gpui-component, native-window, tray, notification, single-instance, and updater dependencies.
- Declare `[[bin]] name = "stereodrome"`, `path = "src/main.rs"`, and `required-features = ["gpui-ui"]`.
- Make `src-tauri/Cargo.toml` depend on `stereodrome-desktop = { path = "../crates/stereodrome-desktop", default-features = false }`. Tauri therefore links only the backend library, never a second UI toolkit or event loop.
- Keep package selection explicit in commands: `-p stereodrome` means the shipping Tauri package; `-p stereodrome-desktop` means the new package.

After Phase 8 removes Tauri, make `gpui-ui` the default/unconditional desktop dependency set, remove `required-features`, and delete the transitional feature split. Do not keep a backend-only feature matrix after there is only one desktop consumer.

### Target file layout

Move existing modules rather than rewriting them. The target layout is:

```text
crates/
├── stereodrome-audio/                 # unchanged shared audio/DSP engine
├── stereodrome-core/
│   └── src/
│       ├── schema.rs                  # pub DESKTOP_SCHEMA
│       ├── schema.sql                 # exact moved canonical SQL
│       └── queue.rs                   # unchanged shared queue engine
└── stereodrome-desktop/
    ├── Cargo.toml
    ├── packager.toml                  # cargo-packager metadata
    ├── assets/                        # desktop icons and bundled UI assets
    └── src/
        ├── lib.rs                     # exports backend types; no GPUI imports
        ├── main.rs                    # feature-gated GPUI binary entry
        ├── backend.rs                 # DesktopBackend bootstrap/shutdown
        ├── state.rs                   # concrete DesktopState ownership graph
        ├── paths.rs                   # DesktopPaths and legacy identity
        ├── store.rs                   # mutex JSON object store
        ├── events.rs                  # watches and durable DesktopEvent
        ├── error.rs
        ├── credentials.rs
        ├── lastfm.rs
        ├── client/                    # moved Subsonic client thread/handle/messages
        ├── db/                        # desktop migrations and queue persistence
        ├── search/                    # moved Tantivy IndexManager
        ├── cache/                     # moved audio/cover cache and locations
        ├── audio/                     # moved desktop player/orchestration wrappers
        ├── operations/                # command bodies, now direct backend methods
        │   ├── auth.rs
        │   ├── library.rs
        │   ├── playlist.rs
        │   ├── search.rs
        │   ├── queue.rs
        │   ├── playback.rs
        │   ├── normalization.rs
        │   ├── cover_art.rs
        │   ├── cache.rs
        │   ├── settings.rs
        │   └── lastfm.rs
        └── ui/                        # all modules gated by gpui-ui
            ├── mod.rs
            ├── app.rs                 # GPUI startup and ordered quit
            ├── model.rs               # one entity shared by every window
            ├── actions.rs             # actions! and keybindings
            ├── theme.rs
            ├── windows.rs             # main/mini/nano/cover-art constructors
            ├── native/
            │   ├── media.rs
            │   ├── tray.rs
            │   ├── notifications.rs
            │   ├── single_instance.rs
            │   └── updater.rs
            └── views/
                ├── auth.rs
                ├── library.rs
                ├── playlists.rs
                ├── queue.rs
                ├── transport.rs
                ├── mini_player.rs
                ├── cover_art.rs
                └── settings.rs
```

`operations` is a module grouping, not a trait layer. Keep helpers beside the moved algorithm when splitting a current command file would obscure invariants. In particular, move `src-tauri/src/commands/playback.rs` and `src-tauri/src/audio/player.rs` last and preserve their coordination.

### Ownership and call flow

- `DesktopBackend::open(DesktopPaths)` creates the Tokio runtime, JSON stores, Subsonic handle, SQLite connection, audio player, queue, search index, Last.fm tracker, event channels, and worker registry. It returns a backend only after schema migration and persisted volume/queue restoration succeed.
- `DesktopBackend::state() -> Arc<DesktopState>` is the single ownership handle used by direct operations and temporary Tauri wrappers.
- GPUI creates one `Entity<DesktopModel>`. Main, mini/nano, and cover-art roots all hold that entity instead of copying playback, queue, navigation, or settings state.
- UI actions spawn backend futures on the owned Tokio runtime. Results return to GPUI through the GPUI foreground executor/context update path proven in Phase 0; no GPUI entity or window handle crosses onto a Tokio worker.
- The model subscribes once to playback/spectrum watches and the durable event receiver. Windows render model state and never subscribe independently.
- Shell-owned media, tray, window, prompt, notification, and updater actions call the model/backend directly. They are not sent through backend events.
- Shutdown is one idempotent `DesktopBackend::shutdown` path. `Drop` is a last-resort guard, not the normal lifecycle.

### Architecture invariants

- No `tauri`, `AppHandle`, `tauri::State`, webview, JavaScript, or string event name appears in the backend library.
- No SQL, cache, sync, search, playlist, queue, playback, gapless, crossfade, normalization, or Last.fm algorithm is reimplemented in GPUI.
- No GPUI type appears in a public backend type.
- Only one native UI/event loop exists in each process.
- Mobile code and FFI continue to compile against the same `StereodromeCore` API.

## Dependency baseline and instability policy

GPUI and gpui-component are pre-1.0, revision-coupled dependencies. Treat the pair as source dependencies, not independently upgradable crates.

### Pinned baseline

The baseline inspected on 2026-07-16 is:

| Item                      | Required value                                 |
| ------------------------- | ---------------------------------------------- |
| Rust toolchain            | `1.95.0`                                       |
| Zed repository            | `https://github.com/zed-industries/zed`        |
| Zed revision              | `1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba`     |
| `gpui`                    | `0.2.2` at the Zed revision                    |
| `gpui_platform`           | `0.1.0` at the Zed revision                    |
| gpui-component repository | `https://github.com/longbridge/gpui-component` |
| gpui-component revision   | `031555662e99a1b5a549990b47f246d475b8288a`     |
| `gpui-component`          | unpublished workspace `0.5.2` at that revision |

Add root `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.95.0"
components = ["clippy", "rustfmt"]
profile = "minimal"
```

Use the same **unqualified** Zed Git URL that gpui-component uses. A URL with a query, branch qualifier, fork, or different spelling creates another Cargo source identity and can produce incompatible duplicate GPUI types. The relevant `crates/stereodrome-desktop/Cargo.toml` entries are:

```toml
[features]
default = []
gpui-ui = [
  "dep:gpui",
  "dep:gpui-component",
  "dep:gpui_platform",
  # native shell dependencies listed below
]

[dependencies]
gpui = { git = "https://github.com/zed-industries/zed", rev = "1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba", optional = true, default-features = false }
gpui-component = { git = "https://github.com/longbridge/gpui-component", rev = "031555662e99a1b5a549990b47f246d475b8288a", optional = true }
gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba", optional = true, default-features = false }

[target.'cfg(target_os = "macos")'.dependencies]
gpui = { git = "https://github.com/zed-industries/zed", rev = "1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba", optional = true, default-features = false, features = ["font-kit"] }
gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba", optional = true, default-features = false, features = ["font-kit"] }

[target.'cfg(target_os = "linux")'.dependencies]
gpui = { git = "https://github.com/zed-industries/zed", rev = "1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba", optional = true, default-features = false, features = ["wayland", "x11"] }
gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba", optional = true, default-features = false, features = ["wayland", "x11"] }
```

If Cargo rejects duplicate target-specific declarations for the same dependency, express the same feature union with target-specific workspace dependencies; do not fall back to GPUI defaults on every OS. Windows needs neither macOS `font-kit` nor Linux `wayland`/`x11`.

Commit the root `Cargo.lock`. Before any application port starts, inspect every package whose source is `git+https://github.com/zed-industries/zed` and require the resolved fragment `#1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba`. `cargo tree -d` must not show duplicate `gpui`, `gpui_platform`, `gpui_macros`, or other Zed GPUI packages from different Git source identities.

### Component policy

- Call `gpui_component::init(cx)` once during application initialization.
- Wrap every main, mini/nano, and cover-art window root in `gpui_component::Root`.
- Use gpui-component's accessible `Input`, form controls, dialogs, menus, lists, tooltips, and bundled icon assets before creating a local equivalent.
- Do not enable optional inspector, decimal, tree-sitter, tree-sitter-language, editor, or chart features. Do not port the Markdown updater notes as an embedded editor; render the small trusted release-note subset with ordinary text/link elements.
- Keep custom GPUI elements limited to Stereodrome-specific library cards, song rows, transport, spectrum, and window chrome.

### Upgrade rule

Do not track a moving branch. Change Rust, the Zed revision, and the gpui-component revision together in one dedicated dependency change only after the complete parity suite passes at the current pins. The dependency-only change must pass native debug and release builds on macOS, Windows, and Linux before feature work resumes.

If either pinned revision fails the Phase 0 native matrix, stop application work. Select the newest mutually compatible Zed/gpui-component commit pair that passes all three OSes, update both SHAs and the Rust pin in this document and the manifests, and commit the resulting lockfile as one change.

## Ordered migration phases

Phases are serial. A phase begins only after every acceptance criterion in the previous phase passes. “Deletion gate” means the earliest point at which named code may be removed; it is not optional cleanup.

### Phase 0 — Feasibility and contract lock

**Depends on:** the pinned dependency baseline only. The production Tauri app remains unchanged and shippable.

**Current anchors**

- `src-tauri/tauri.conf.json` defines the 800×600 main window and 800×600 minimum.
- `src-tauri/src/lib.rs::run` owns the current native event loop and platform integration.
- `src-tauri/src/commands/windowing.rs` records main, 320×72 mini, 30×30 nano, and macOS panel requirements.
- `src/lib/components/library/SongList.svelte`, `AlbumGridView.svelte`, and `ArtistGridView.svelte` establish the large-list shapes.

**Exact edits**

1. Add the workspace member, pinned toolchain, `crates/stereodrome-desktop/Cargo.toml`, backend-safe `lib.rs`, and the `gpui-ui`-gated `stereodrome` binary. Do not move product code yet.
2. Initialize `gpui_component::init`; open a main window at 800×600 with a minimum size of 800×600; wrap its root in `gpui_component::Root`.
3. Add one removable `ui/feasibility.rs` screen that proves:
   - gpui-component `Input` accepts Latin text, non-Latin IME composition, selection, copy/paste, and keyboard focus;
   - AccessKit exposes a named text field, buttons, selected/disabled states, list/list-item roles, and visible focus movement;
   - raster cover art and bundled SVG icons render;
   - `uniform_list` scrolls and selects fixed-height song rows;
   - a virtualized list of fixed-height card rows presents an album grid without constructing every card;
   - application menus and a gpui-component context menu dispatch typed actions;
   - `App::prompt_for_paths` selects one directory;
   - a Tokio task result is delivered to and applied on GPUI’s foreground executor;
   - main, `WindowKind::Floating` mini, and cover-art windows open and close while sharing one entity;
   - raw-window-handle access returns the native handle; on Windows that handle initializes `souvlaki`;
   - `tray-icon` menu activation wakes the GPUI loop and dispatches an action.
4. Add native debug and release build jobs for macOS, Windows, and Linux without replacing `.github/workflows/release.yml::build`.
5. On macOS, prove the raw native-window/`objc2` path can apply nonactivating panel and all-spaces behavior required by the existing mini/nano player.

**Edge handling**

- Run Linux under both X11 and Wayland when available. Install and exercise the existing GTK/appindicator packages plus the portal/file-picker runtime packages.
- Picker cancellation produces `None` and no state change. Picker startup/runtime errors appear visibly on the feasibility screen.
- If GPUI’s Linux picker still returns its documented startup error on a supported environment after portal packages are installed, use `rfd` `0.17.2` `AsyncFileDialog::pick_folder` for this folder path only. Keep identical cancel/error behavior and add no dialog abstraction.
- A tray callback may originate off the GPUI thread; it must wake/dispatch onto GPUI rather than mutate an entity directly.
- If the macOS panel flags are unavailable publicly, obtain the native window and set the missing flags with `objc2`/AppKit. A normal activating window is not an acceptable fallback.

**Acceptance criteria**

- `cargo build -p stereodrome-desktop --features gpui-ui --bin stereodrome` passes natively in debug and release mode on macOS, Windows, and Linux.
- A human records the expected and observed result for every feasibility control on all three OSes.
- Linux tray activation and folder selection work at runtime; Windows records a valid HWND and working `souvlaki` initialization; macOS records nonactivating/all-spaces panel behavior.
- The existing Tauri checks and package build still pass.

**Deletion gate**

- Delete only `ui/feasibility.rs` after its probes are represented by the real shell and the Phase 3–6 acceptance checks.
- Do not begin backend or product UI migration if any native matrix/runtime criterion fails.

### Phase 1 — Identity, schema, settings, and lifecycle

**Depends on:** Phase 0 accepted on all three OSes.

**Current anchors**

- `src-tauri/tauri.conf.json::identifier` is `dev.xikxp1.stereodrome`.
- `src-tauri/src/db/mod.rs::{SCHEMA,run_migrations,get_db_path}` owns desktop schema/migrations.
- `crates/stereodrome-core/src/db.rs::SCHEMA` currently reaches into `src-tauri/src/db/schema.sql`.
- `src-tauri/src/credentials.rs` defines the existing keyring identity.
- `src-tauri/src/commands/settings.rs`, `ui_state.rs`, and `cache/locations.rs` define JSON files/keys and cache paths.
- Perpetual work starts in `client/thread.rs`, `audio/player.rs`, `commands/library.rs::start_library_sync_scheduler`, `commands/nowplaying.rs::start_now_playing_emitter`, and `lastfm.rs::start_lastfm_retry_scheduler`.

**Exact edits**

1. Move `src-tauri/src/db/schema.sql` byte-for-byte to `crates/stereodrome-core/src/schema.sql`. Add `schema.rs` with public `DESKTOP_SCHEMA`; make both `stereodrome-core::db` and `src-tauri/src/db/mod.rs` use it. Keep the desktop `run_migrations` body and order unchanged.
2. Add `DesktopPaths`. Derive the candidate profile with `directories` `6.0.0` `BaseDirs::data_dir().join("dev.xikxp1.stereodrome")`; expose paths for `stereodrome.db`, `search_index`, `settings.json`, `state.json`, default cache root, `audio_cache`, and `cover_cache`.
3. Before opening SQLite, compare the candidate directory to the path used by an installed Tauri build on that OS and open a disposable copied profile there. If they differ, implement that OS’s legacy known-folder calculation directly. Never create, migrate, rename, or merge a second profile.
4. Add the concrete mutex-protected JSON object store specified in “Backend cut.” Replace Tauri store reads/writes behind the current Tauri commands while preserving unknown keys.
5. Replace detached perpetual work with a cancellation token/flag plus retained join handle per backend worker. The backend registry covers the Subsonic client, playback position/spectrum emitters, library scheduler, current now-playing poller until its later deletion, and Last.fm retry scheduler. Shell-owned media/tray workers receive their own retained handles when they move in Phase 6.
6. Add one idempotent shutdown method that cancels first, requests component shutdown, then joins handles outside state locks.

**Edge handling**

- Reject non-object JSON as a visible startup/settings error; do not overwrite it.
- A missing JSON file means an empty object plus existing defaults. Unknown top-level keys survive every write.
- Create directories only after the legacy path has been proven. Database/schema migration failures leave the source profile untouched and abort startup.
- Joining cannot hold SQLite, queue, audio, settings, event, or GPUI locks. Apply bounded waits only to OS APIs that cannot be interrupted, log the timeout, and retain data.

**Acceptance criteria**

- Hash comparison proves the moved `schema.sql` is unchanged.
- Fresh profile startup creates the existing names and defaults.
- A disposable copy of a real profile opens with the same server identity, library counts, queue, volume, mini position, settings, cache location, Tantivy results, and keyring session on every OS.
- Repeated JSON writes preserve an injected unknown top-level key and leave parseable files after forced process interruption.
- Ordered shutdown leaves no retained worker running and restart reopens the database cleanly.
- Shared/core/mobile checks pass after the schema move.

**Deletion gate**

- Remove `src-tauri/src/db/schema.sql` only after both consumers use `stereodrome_core::DESKTOP_SCHEMA`.
- Remove each detached startup path only when its retained replacement is used by Tauri and covered by shutdown verification.

### Phase 2 — Tauri-free backend

**Depends on:** copied-profile and lifecycle acceptance from Phase 1.

**Current anchors**

- `src-tauri/src/state.rs::AppState` is the concrete ownership graph.
- `src-tauri/src/lib.rs::run` shows startup order and the complete command registration.
- `src-tauri/src/commands/*.rs` contains the business bodies.
- `src-tauri/src/commands/playback.rs` and `src-tauri/src/audio/player.rs` contain the highest-risk `AppHandle` coupling.

**Exact edits**

1. Move, in order, `error`, `client`, `credentials`, desktop DB/queue persistence, search, cache, state, settings models/store, Last.fm, library/sync, playlists, queue, normalization, cover art, and auth into `stereodrome-desktop`.
2. Convert command bodies into plain functions/methods taking `&DesktopState` or `Arc<DesktopState>`, explicit `DesktopPaths`, stores, runtime handle, and `DesktopEvents`. Remove `#[tauri::command]`, `AppHandle`, and `tauri::State` from moved code.
3. Replace path/plugin lookups with `DesktopPaths`/the JSON stores; replace Tauri emits with typed event methods; replace `tauri::async_runtime` with the owned Tokio runtime.
4. Leave a thin wrapper per registered command in `src-tauri/src/commands`. A wrapper extracts `State<'_, DesktopBackend>`, awaits/calls the direct operation, converts only the outer error shape, and forwards typed backend events to the existing string event names. It contains no SQL or business branching.
5. Have `src-tauri/src/lib.rs::run` construct/manage `DesktopBackend`, then initialize Tauri-only media, tray, plugins, and windows around it.
6. Move playback last. Preserve `fetch_song_data`, prefetch, original suffix/cache behavior, gapless eligibility, crossfade handoff, navigation atomic guard, queue persistence, loudness analysis, Last.fm side effects, and backend-owned end-of-track advancement. Replace only shell/event dependencies.
7. Remove the unreachable backend `now-playing` polling worker/event after confirming no reachable frontend imports `src/lib/stores/nowplaying.svelte.ts`.

**Edge handling**

- A synchronous UI-triggered operation may emit an event before returning; event delivery must never wait for the same UI call to finish.
- Preserve lock ordering and keep awaits outside mutex guards.
- Existing Tauri payloads remain `snake_case` and byte/field compatible throughout coexistence.
- Manual offline mode continues to block network sync, scan, discovery, updater, and Last.fm network calls while allowing local library/cache playback.

**Acceptance criteria**

- The backend library builds with `--no-default-features` and contains no Tauri dependency or import.
- Every command registered in `src-tauri/src/lib.rs` reaches a moved direct operation or is explicitly shell-only (`windowing`, notification presentation, tray update, updater).
- Tauri root checks, Rust clippy/tests/build, and a full Tauri parity smoke run pass against the moved backend.
- Backend tests preserve existing SQL, cache, sync, search, playlist duplicate-position, queue, playback, normalization, and Last.fm behavior.

**Deletion gate**

- Delete a Tauri business body only after its wrapper calls the moved implementation and the production shell passes.
- Do not remove Tauri commands/events or frontend wrappers yet.

### Phase 3 — GPUI application model and bootstrap

**Depends on:** all reachable backend behavior is Tauri-free.

**Current anchors**

- `src/routes/+page.svelte` owns top-level view/auth state and keyboard handling.
- `src/lib/stores/connection.svelte.ts`, `playback.svelte.ts`, `queue.svelte.ts`, and `spectrum.svelte.ts` project backend state.
- `src-tauri/src/lib.rs::{focus_main_window,run}` owns single-instance focus, close-to-tray, startup, and exit.

**Exact edits**

1. Create one `Entity<DesktopModel>` after `DesktopBackend::open`. Store auth, connectivity/offline gate, navigation, selection, playback, queue, spectrum, settings, updater, window-presence, and quitting state in it.
2. Subscribe once to playback and spectrum watches and once to the durable event receiver. Apply all updates through GPUI context updates and call `cx.notify()` after model changes.
3. Port session restore, connect, disconnect, configured-but-offline startup, and manual-offline transitions as direct backend actions. A configured offline profile opens the local library rather than the login screen.
4. Define `actions!` and keybindings for every current shortcut; bind actions at the model/root so menus, tray/media callbacks, and windows share dispatch.
5. Port the current light theme tokens with gpui-component theme support. Preserve focus indicators and minimum contrast; visual redesign is out of scope.
6. Add `open_main_window(model, cx)`. Main close removes only the window handle and keeps backend/model/native services alive. Tray Show and second-instance messages both call the helper, which recreates, stores, activates, and focuses the main window.
7. Implement ordered quit: set `quitting` and reject new actions; stop shell-owned media/tray callbacks and workers; call the idempotent `DesktopBackend::shutdown` path specified below; drop remaining native resources; then quit GPUI.

**Edge handling**

- Repeated Show/second-instance messages are idempotent and focus the existing main window.
- A close request during quit is not converted back into close-to-tray.
- Async auth results carry a generation/session identity; stale results after disconnect or another login are ignored.
- Backend/model state survives zero open windows.

**Acceptance criteria**

- Fresh login, successful/failed connect, credential restore, disconnect, network-unavailable restore, and manual offline transitions show the same reachable states as Tauri.
- Closing and recreating main does not stop playback, clear queue/navigation, duplicate event subscriptions, or restart workers.
- A second launch focuses/recreates the first instance on every OS.
- Clean quit drains workers and a subsequent launch restores the profile without database lock/recovery errors.

**Deletion gate**

- Remove temporary feasibility app/model code after these real paths replace it.
- Keep the production Tauri shell.

### Phase 4 — Library vertical slice

**Depends on:** shared GPUI model/auth/bootstrap accepted.

**Current anchors**

- `src/routes/+page.svelte` is authoritative for state-only navigation, offline filtering, detail/back behavior, and view composition.
- `Sidebar.svelte`, `ColumnBrowser.svelte`, `ArtistGridView.svelte`, `AlbumGridView.svelte`, `ArtistAlbumRail.svelte`, `DetailHeader.svelte`, `SongList.svelte`, and `StatusBar.svelte` define reachable library UX.
- `search.svelte.ts`, `playlist.svelte.ts`, `albumList.svelte.ts`, `libraryRefresh.svelte.ts`, and `contextMenu.ts` define reachable state/actions.
- Backend anchors are `operations::{library,search,playlist,cache,cover_art}`.

**Exact edits**

1. Port sidebar navigation in current order: Music, Artists, Albums, Recently Added, Recently Played, Most Played, then playlists. Navigation remains model state; do not add a router.
2. Port the Music column browser and fixed-height song rows with `uniform_list`. Preserve genre → artist → album filtering, selected/playing/downloaded states, multi-disc ordering, keyboard selection, locate-current behavior, and artist/album navigation.
3. Port artist/album/recent grids as a virtualized list of fixed-height card rows. Compute cards per row from available width; instantiate only visible rows. Preserve scroll offsets when entering/backing out of detail.
4. Port artist and album detail headers, artist album rail, cover-art viewer, and album → artist / song → artist/album back paths.
5. Port search through direct Tantivy calls. Debounce input, increment a query generation, and apply a result only if its generation and configured account still match. Disable selection/play actions while the accepted query is pending.
6. Preserve offline filtering: configured offline sessions show only cached song IDs and artists/albums represented by those songs; server-only recent views do not issue network calls.
7. Port playlists: cached list/load, create, rename, delete, select, add, remove, save/remove offline, and reconcile saved playlists. Keep selection/removal by playlist **position**, not song ID, so duplicate song occurrences remain independently actionable.
8. Use component `Input`, dialogs, and context menus on all OSes. Expose identical queue/add-next/playlist/navigate/remove actions; do not add a second platform-specific context-menu implementation.
9. Port sync/scan status, incremental/full reconcile actions, scheduled status, content refresh, cache-change refresh, loading/error/empty states, counts, durations, and sizes.
10. Delete rather than port unreachable `src/lib/db/collections.ts`, `src/lib/db/queryClient.ts` if unreferenced with it, `src/lib/components/library/AlbumGrid.svelte`, `ArtistList.svelte`, `src/lib/stores/nowplaying.svelte.ts`, and `src/lib/components/MarqueeText.svelte`.

**Edge handling**

- Empty search clears results and invalidates an in-flight query.
- Account/disconnect changes invalidate library/search generations before clearing model data.
- Network-only discovery actions are disabled with an explanation in manual/configured offline mode; local detail and playback remain usable.
- Picker/context/dialog cancellation makes no mutation. Destructive playlist delete restores focus to the nearest remaining playlist or New Playlist control.

**Acceptance criteria**

- Every top-level route, detail/back path, offline filter, selection, context action, and status state matches the current Tauri UI on the manual fixture.
- Scrolling a large song list/grid has bounded rendered rows/cards and no eager all-card allocation.
- Rapid search input never displays an older result after a newer query.
- Removing one of two duplicate playlist occurrences removes only the selected position.
- Screen-reader names/roles/state and keyboard focus order are verified for navigation, grids/lists, search, context menus, and dialogs.

**Deletion gate**

- The named unreachable files may be deleted once import search and root Svelte checks prove they have no reachable caller.
- Keep all reachable Svelte library code until Phase 8.

### Phase 5 — Playback vertical slice

**Depends on:** library selection and direct backend calls accepted.

**Current anchors**

- `TransportBar.svelte`, `NowPlayingCenter.svelte`, `QueuePanel.svelte`, `SpectrumBars.svelte`, and the playback/queue/spectrum stores define reachable playback UI.
- `src/routes/mini-player/+page.svelte` defines 320×72 mini and 30×30 nano behavior.
- `src-tauri/src/commands/{playback,queue}.rs`, `audio/player.rs`, `cache/audio.rs`, and `lastfm.rs` own playback invariants and side effects.

**Exact edits**

1. Project the playback watch into transport title/artist/album/art, play/pause, elapsed/duration, seek, volume, and playing state. Persist volume through the backend only at the same current interaction boundary.
2. Port queue projection, add/insert-next/remove/clear/reorder, item play, locate current, prepared next item, next/previous, shuffle, repeat cycle/set, and reroll. Backend queue state remains authoritative.
3. Port spectrum from its watch snapshot; disabling it stops repaint work but does not alter audio analysis semantics.
4. Bind all current shortcuts: Space, Enter, modifier+Left/Right, Shift+Left/Right, modifier+Up/Down, M, S, R, Q, V, D, modifier+K, and modifier+Comma. Inputs consume text keys; Escape restores/clears input focus as today.
5. Keep scrobble/now-playing and notification decisions owned once. Backend playback transitions own Last.fm submission/queueing; the shell observes the accepted track transition and presents at most one notification according to settings/focus/mini state.
6. Create/recreate main, 320×72 mini, 30×30 nano, and cover-art windows from the shared model. Persist mini/nano logical coordinates, clamp them to a current monitor work area, preserve top-right default placement, hover/focus controls, next-song display setting, and mode switching.
7. Preserve the existing backend’s original-suffix fetch/cache path, LRU eviction, prefetch, loudness-on-demand, gapless eligibility, multi-disc ordering, crossfade timing/manual-next setting, DSP chain, navigation guard, end-of-track queue advancement, and queue persistence. GPUI renders outcomes; it does not reproduce these decisions.

**Edge handling**

- Seek clamps to `[0,duration]`; volume remains finite and clamped to the existing range.
- A removed monitor relocates saved mini/nano coordinates to the active/fallback monitor without overwriting the saved value until the user moves the window.
- Mini/nano close removes that window only; reopening uses the shared current track/queue immediately.
- Gapless/crossfade handoffs must not double-advance or double-scrobble when a playback-ended signal races a manual action.

**Acceptance criteria**

- The fixture proves play/pause/stop, seek, volume/restart persistence, queue mutations/reorder/locate, next/previous, shuffle/repeat/reroll, keyboard actions, spectrum, and end-of-queue.
- Same-album consecutive tracks transition gaplessly; configured crossfade and manual-next crossfade use the existing timing; other tracks use the unchanged normal transition.
- Original suffix/cache hit/prefetch/eviction and every DSP toggle produce the same backend-observable state as Tauri.
- Mini/nano sizes, position restoration/clamping, hover/focus controls, next-song setting, mode switch, close/reopen, and cover viewer pass on each OS.
- One qualifying track produces at most one now-playing update, notification, and scrobble side effect.

**Deletion gate**

- No playback algorithm may be deleted or replaced; only Tauri wrappers become removable in Phase 8.
- Remove any frontend polling superseded by watches only when the GPUI slice and shipping Tauri wrapper both retain backend-owned advancement.

### Phase 6 — Settings and native services

**Depends on:** library and playback vertical slices accepted.

**Current anchors**

- `SettingsModal.svelte` defines section order and all reachable controls.
- `src-tauri/src/commands/{settings,normalization,cache,notifications,windowing,updater}.rs`, `media/controls.rs`, and `tray/manager.rs` define current behavior.
- `updater.svelte.ts`, `notifications.svelte.ts`, `mediaControls.svelte.ts`, and `trayControls.svelte.ts` define shell presentation/dispatch.

**Exact edits**

1. Port Settings in the current order: Updates; Server and scan; Library Sync; Display; Desktop Notifications; Last.fm; Playback including gapless/crossfade/binaural/EQ; Volume Normalization including dynamics/analysis; Audio Cache.
2. Preserve every default, clamp, enum, unit, enable/disable dependency, progress/status field, scheduler timestamp locale/clock formatting, destructive confirmation, and retry/error display.
3. Use gpui-component controls/dialogs and restore focus to the opener after every dialog. Use `App::open_url` for Last.fm authorization and release links.
4. Use `App::prompt_for_paths` with `{ files: false, directories: true, multiple: false }` for cache root. Cancellation leaves the root unchanged; failure produces a visible error; success calls the existing move/no-overwrite algorithm and displays its summary.
5. Wire `souvlaki`, `tray-icon`, `notify-rust`, `single-instance` + `interprocess`, platform windows, and `cargo-packager-updater` directly to typed model/backend actions using the choices below.
6. Keep native resources shell-owned. Their callbacks dispatch model actions; they do not create a parallel state store or backend event family.

**Edge handling**

- Destructive cache clear, normalization clear, disconnect, and cache-root reset require an accessible confirmation and make no change on cancel.
- Settings save failures leave the last persisted value visible plus an error; they do not optimistically claim success.
- Notification images are optional; a missing cover path sends text rather than failing playback.
- Last.fm and updater network actions obey manual offline mode.

**Acceptance criteria**

- Every control is compared against Tauri with default, changed, restart-restored, disabled, error, and cancellation states.
- Screen readers announce control names, roles, values, selected/disabled state, progress, validation errors, dialog title/body/actions, and restored focus on all OSes.
- Media keys/OS metadata, tray menu/actions/state, notifications with available cover image, single-instance focus, URL open, folder prompt, and updater check/download/install paths work natively.
- JSON unknown-key and atomic-write checks still pass after changing every settings section.

**Deletion gate**

- Do not remove Tauri plugins/native services until GPUI package/update rehearsal passes.

### Phase 7 — Packages, updater bridge, and release rehearsal

**Depends on:** full GPUI behavior parity through Phase 6.

**Current anchors**

- `src-tauri/tauri.conf.json::bundle` defines identity, descriptions, targets, icons, updater key/endpoint, and Linux templates.
- `.github/workflows/release.yml` defines the three-OS matrix, signing/notarization inputs, artifact names, draft release, release notes/links, Last.fm compile inputs, and mobile jobs.
- `mobile/app.json` consumes `src-tauri/icons/icon.png` and the Android foreground icon.

**Exact edits**

1. Copy Expo-consumed icons to `mobile/assets/icon.png` and `mobile/assets/adaptive-icon.png`; update `mobile/app.json`. Copy retained desktop icons/templates into `crates/stereodrome-desktop/assets`; do not delete source renditions until both consumers build.
2. Configure `cargo-packager` with product `Stereodrome`, identifier `dev.xikxp1.stereodrome`, publisher/author, MIT license, Music category, homepage, existing short/long descriptions, icons, Linux desktop metadata, and the `stereodrome` binary.
3. Generate DMG/app (including universal macOS), NSIS/MSI, AppImage/DEB, and signed updater artifacts with `cargo-packager` `0.11.8`; generate RPM with `cargo-generate-rpm` `0.21.0`. Add deterministic rename/copy steps only where needed to retain the release links’ current artifact names.
4. Integrate `cargo-packager-updater` `0.2.3` with the existing public key and release endpoint contract. Never reuse a downloaded artifact until signature and target/version checks pass.
5. Add GPUI canary package jobs on macOS, Ubuntu, and Windows while the existing Tauri `build` job still publishes production artifacts. Keep `LASTFM_API_KEY` and `LASTFM_SHARED_SECRET` compile inputs, Apple/Windows signing, macOS notarization, draft release composition, release notes, and both mobile jobs.
6. Publish separate Tauri-compatible and GPUI updater manifests during coexistence. Rehearse clean installs and signed Tauri → first GPUI → next GPUI updates on each OS using disposable VMs/users and copied profiles.

**Edge handling**

- If updater artifact/manifest formats are incompatible on an OS, leave that OS’s old manifest pointing to the final Tauri release and require one signed installer upgrade. Do not attempt a destructive in-place conversion.
- An update failure keeps the installed binary/profile usable and reports a retryable error.
- macOS universal packaging must test both architectures; Linux package tests include desktop entry, icon, appindicator/GTK dependencies, audio, and file picker; Windows tests MSI and NSIS upgrade/uninstall behavior.

**Acceptance criteria**

- Clean DMG/app, MSI, NSIS, AppImage, DEB, and RPM installs launch the GPUI binary with correct identity/icons and no web runtime dependency.
- Artifact names exactly satisfy the links composed in `.github/workflows/release.yml`.
- Signed update chains pass on every compatible OS; any installer-only exception is documented in release notes and tested.
- Tauri production release, GPUI canaries, Last.fm compile inputs, draft release, Android, and iOS jobs all remain green during coexistence.

**Deletion gate**

- Remove source icons only after both relocated mobile and desktop assets build/package.
- Do not promote GPUI or delete Tauri until one full canary cycle and all install/update rehearsals pass.

### Phase 8 — Clean cutover

**Depends on:** Phase 7 release rehearsal and the complete parity walkthrough signed off on all three OSes.

**Exact edits**

1. Make `crates/stereodrome-desktop` the only desktop package/binary. Remove the transitional feature split and make GPUI dependencies unconditional for the binary.
2. Update `scripts/set-version.mjs` and release version validation from `src-tauri/Cargo.toml` to `crates/stereodrome-desktop/Cargo.toml`. Preserve shared/mobile version checks.
3. Replace CI/release Tauri build/cache/action steps with native Cargo, cargo-packager, updater, and RPM steps. Preserve artifact names/release links, signing/notarization, Last.fm environment, draft release, and mobile jobs.
4. Update `.github/dependabot.yml` to the root workspace, Rust cache roots to `.`, developer commands to explicit `-p stereodrome-desktop`, and updater/cache references to the new package/assets without changing the profile directory.
5. Remove Tauri commands, wrappers, plugins, build script/config, capabilities, generated files, bundle templates, and the stale nested `src-tauri/Cargo.lock`; then remove all `src-tauri`.
6. Remove reachable Svelte/root web source, `static`/`build` output, root web dependencies/lockfile, and Svelte/Vite/Tailwind/TypeScript/ESLint configuration that no remaining tool uses. Remove root `package.json` if no non-web script remains; run versioning directly with `bun scripts/set-version.mjs`.
7. Remove only icon renditions proven unused after mobile and cargo-packager consume their relocated assets.
8. Remove obsolete Tauri-specific checks/docs/permissions and the coexistence updater manifest after the supported transition window.

**Keep**

- Root `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, license, repository docs, `.github`, and `.gitignore`.
- `crates/stereodrome-audio`, `crates/stereodrome-core`, `crates/stereodrome-ffi`, and `crates/stereodrome-desktop`.
- All `mobile` source/native bridge files and mobile lockfile.
- `scripts/set-version.mjs`, `build-mobile-rust.sh`, `build-and-check-mobile-rust.sh`, and `bun-install-with-retry.sh` while referenced by mobile CI.
- Existing desktop profile/keyring data in place.

**Edge handling**

- Search the repository for `tauri`, `src-tauri`, `@tauri-apps`, `invoke(`, and old string event names. Remaining hits must be historical prose or deliberately retained migration tests, not runtime/build code.
- Never delete user data, move a profile, clear a keyring entry, or reset settings during uninstall/cutover.
- Keep the bridge installer path available for users who skip the final Tauri auto-update window.

**Acceptance criteria**

- A clean checkout runs the final desktop, shared Rust, and mobile checks with no Tauri/web toolchain installation.
- The full manual walkthrough passes on clean and copied profiles for macOS, Windows, and Linux packages.
- Last Tauri → first GPUI (or documented signed installer) → next GPUI update succeeds without data/keyring/cache/search loss.
- Repository search finds no runtime/build dependency on Tauri or the root Svelte app.

**Deletion gate**

- This is the final deletion gate. Merge deletion only with three-OS package/update evidence and parity sign-off attached.

## Backend cut

This section fixes the backend API before code moves. Keep it concrete; do not generalize it into providers or repositories.

### `DesktopPaths`

`DesktopPaths::detect` computes paths but does not open, create, or migrate anything:

```rust
pub struct DesktopPaths {
    pub data_dir: PathBuf,
    pub database: PathBuf,
    pub search_index: PathBuf,
    pub settings: PathBuf,
    pub state: PathBuf,
    pub default_cache_root: PathBuf,
    pub audio_cache: PathBuf,
    pub cover_cache: PathBuf,
}

impl DesktopPaths {
    pub fn detect() -> Result<Self, DesktopError> {
        let data_dir = directories::BaseDirs::new()
            .ok_or(DesktopError::NoDataDirectory)?
            .data_dir()
            .join("dev.xikxp1.stereodrome");
        Ok(Self::from_data_dir(data_dir))
    }
}
```

`from_data_dir` derives the existing leaf names: `stereodrome.db`, `search_index`, `settings.json`, `state.json`, and the default cache directories `audio_cache`/`cover_cache`. If `cache_root` is set, audio and cover cache directories derive from that persisted root exactly as today.

The caller must prove `data_dir` equals Tauri’s installed app-data path on that OS using a disposable copied profile **before** `DesktopBackend::open` creates directories or opens SQLite. On mismatch, replace only `detect` for that OS with the legacy known-folder calculation. Do not auto-move or merge data.

### JSON object store

Use one concrete `JsonStore` per file:

```rust
pub struct JsonStore {
    path: PathBuf,
    values: Mutex<serde_json::Map<String, serde_json::Value>>,
}
```

Required behavior:

1. `open` reads the whole file once. Missing means an empty map. Invalid JSON or a non-object root is an error and is never overwritten.
2. Typed reads deserialize only the requested key and apply the existing serde defaults/clamps in the settings model.
3. Typed writes replace only the requested key in the in-memory map, preserving all unknown top-level keys.
4. A save serializes the complete map to a uniquely named sibling temporary file, writes all bytes, calls `sync_all`, and atomically replaces the target. Use rename-over-target on Unix; use `ReplaceFileW`/`MoveFileExW` with replace/write-through semantics on Windows. Sync the parent directory where the OS supports it.
5. Hold the store mutex across map mutation and durable replacement so concurrent settings writes cannot lose keys. Never call event subscribers or backend operations while holding it.
6. On replacement failure, retain the old target, remove only the temporary file, restore the in-memory value to the last durable map, and return the error.

Do not split settings into new files. Preserve these keys:

| File            | Top-level keys                                                                                                |
| --------------- | ------------------------------------------------------------------------------------------------------------- |
| `settings.json` | `normalization`, `playback`, `notification`, `sync`, `connectivity`, `max_cache_size`, `cache_root`, `lastfm` |
| `state.json`    | `volume`, `mini_player_position`                                                                              |

### Backend ownership

The public backend surface is:

```rust
pub struct DesktopBackend {
    state: Arc<DesktopState>,
    runtime: tokio::runtime::Runtime,
    workers: Mutex<Vec<WorkerHandle>>,
    shutdown: AtomicBool,
}

pub struct DesktopState {
    pub paths: DesktopPaths,
    pub settings: JsonStore,
    pub ui_state: JsonStore,
    pub client: SubsonicClientHandle,
    pub db: Mutex<rusqlite::Connection>,
    pub audio_player: Mutex<AudioPlayer>,
    pub queue: Mutex<PlayQueue>,
    pub search_index: Mutex<Option<IndexManager>>,
    pub lastfm_tracker: Mutex<LastfmPlaybackTracker>,
    pub analysis_progress: Mutex<Option<AnalysisProgress>>,
    pub navigating: AtomicBool,
    pub lastfm_retry_running: AtomicBool,
    pub events: DesktopEvents,
}
```

Use the current `AppState` fields and add only explicit paths/stores/events/lifecycle ownership. `WorkerHandle` is a small enum over retained Tokio tasks and OS threads with `cancel/request_shutdown` and `join`; it is not a generic executor abstraction.

`DesktopBackend::open` order is fixed:

1. validate/provision the proven profile and cache directories;
2. open both JSON stores without writing them;
3. spawn the Subsonic client/runtime resources;
4. open SQLite and run the unchanged schema/migrations;
5. create audio, search, queue, Last.fm, and event state;
6. restore queue and persisted volume;
7. start retained position/spectrum, sync, and retry workers;
8. return the usable backend.

If a step fails, unwind already-created resources in reverse order and leave persisted files unchanged.

### Typed event contract

High-rate “latest value” data uses Tokio watch channels:

```rust
pub struct DesktopEvents {
    playback_tx: tokio::sync::watch::Sender<PlaybackState>,
    spectrum_tx: tokio::sync::watch::Sender<SpectrumData>,
    durable_tx: tokio::sync::mpsc::UnboundedSender<DesktopEvent>,
}
```

- `playback_tx` always contains the latest `PlaybackState`.
- `spectrum_tx` always contains the latest `SpectrumData`, including the idle/default snapshot.
- The shell obtains watch receivers with `subscribe`.
- Low-frequency ordered state transitions use one unbounded MPSC queue. Each backend process has one shell consumer; `DesktopBackend::take_event_receiver` succeeds once.
- Unbounded delivery is deliberate: a synchronous UI call can emit before returning without blocking behind its own UI consumer. The listed events are low-frequency; do not send position or spectrum frames through this queue.

The durable event enum is exactly:

```rust
pub enum DesktopEvent {
    PlaybackEnded,
    QueueChanged(QueueState),
    QueueEnded,
    AudioCacheChanged(AudioCacheChangedEvent),
    NormalizationProgress(AnalysisProgress),
    LibrarySyncStatusChanged(LibrarySyncStatus),
    LibraryContentUpdated(LibraryContentUpdatedEvent),
    PlaybackSettingsChanged(PlaybackSettings),
    ConnectivitySettingsChanged(ConnectivitySettings),
    SyncSettingsChanged(SyncSettings),
}
```

Media commands, tray commands, window open/close/focus, notifications, prompts, URL opening, single-instance messages, and updater actions are typed **shell-owned actions**, not `DesktopEvent` variants. The removed `now-playing` poll event is not represented.

During coexistence, one Tauri forwarding task owns the receiver and maps variants to existing names/payloads:

| Typed source                  | Temporary Tauri event           |
| ----------------------------- | ------------------------------- |
| playback watch                | `playback-state`                |
| spectrum watch                | `spectrum-data`                 |
| `PlaybackEnded`               | `playback-ended`                |
| `QueueChanged`                | `queue-changed`                 |
| `QueueEnded`                  | `queue-ended`                   |
| `AudioCacheChanged`           | `audio-cache-changed`           |
| `NormalizationProgress`       | `normalization-progress`        |
| `LibrarySyncStatusChanged`    | `library-sync-status-changed`   |
| `LibraryContentUpdated`       | `library-content-updated`       |
| `PlaybackSettingsChanged`     | `playback-settings-changed`     |
| `ConnectivitySettingsChanged` | `connectivity-settings-changed` |
| `SyncSettingsChanged`         | `sync-settings-changed`         |

GPUI does no string mapping; its model matches the enum.

### Business-body cut

Move current command modules in this order:

| Current implementation                               | Target direct operation                                |
| ---------------------------------------------------- | ------------------------------------------------------ |
| `commands/auth.rs`, `client`, `credentials.rs`       | `operations/auth.rs`, `client`, `credentials.rs`       |
| `db`, `commands/library.rs`                          | `db`, `operations/library.rs`                          |
| `search`, `commands/search.rs`                       | `search`, `operations/search.rs`                       |
| `cache`, `commands/cache.rs`, `commands/coverart.rs` | `cache`, `operations/{cache,cover_art}.rs`             |
| `commands/playlist.rs`                               | `operations/playlist.rs`                               |
| `audio/queue.rs`, `db/queue.rs`, `commands/queue.rs` | `audio/queue.rs`, `db/queue.rs`, `operations/queue.rs` |
| `commands/settings.rs`, `commands/ui_state.rs`       | `store.rs`, `operations/settings.rs`                   |
| `lastfm.rs`, `commands/lastfm.rs`                    | `lastfm.rs`, `operations/lastfm.rs`                    |
| `commands/normalization.rs`                          | `operations/normalization.rs`                          |
| `audio/player.rs`, `commands/playback.rs`            | `audio/player.rs`, `operations/playback.rs` (last)     |

Keep function bodies and tests intact where possible. Replace only their inputs and effects: `State` → `&DesktopState`/`Arc<DesktopState>`, `AppHandle` path/store/runtime → explicit owned fields, and `emit` → `DesktopEvents`.

### Shutdown contract

`DesktopBackend::shutdown` is idempotent and executes in this order:

1. atomically disable new backend/model actions;
2. signal cancellation to library, Last.fm, playback snapshot, spectrum, and remaining monitor workers;
3. stop audio playback and prevent another queue advance;
4. call `SubsonicClientHandle::shutdown`;
5. take retained backend handles under the worker mutex, release the mutex, and join every handle;
6. flush any pending settings/state write and drop SQLite/search/cache resources;
7. return control to the shell, which drops remaining native resources and exits GPUI.

No join occurs while holding a backend state lock. A failed worker join is logged and returned in an aggregate shutdown error; it does not skip later joins.

## Current-to-target parity matrix

Every row is a release requirement, not a suggestion.

| Area                             | Current Svelte/Tauri anchor                                                                                                                                                        | GPUI/backend target                                                                                                         | Required parity                                                                                                                                                                              |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Auth/session/offline             | `ServerConnect.svelte`; `stores/connection.svelte.ts`; `commands/auth.rs::{connect_server,restore_session,disconnect_server}`; `commands/settings.rs::manual_offline_enabled`      | `ui/views/auth.rs`; `DesktopModel`; `operations/auth.rs`; `operations/settings.rs`                                          | Fresh connect/error, credential restore, configured offline startup, disconnect, manual offline gates, and account-switch invalidation.                                                      |
| Library sync/search/scan         | `SettingsModal.svelte`; `StatusBar.svelte`; `stores/search.svelte.ts`; `services/libraryRefresh.svelte.ts`; `commands/{library,search}.rs`; `search/IndexManager`                  | `ui/views/{library,settings}.rs`; `operations/{library,search}.rs`; moved `search/IndexManager`; typed sync/content events  | Incremental/full reconcile, scheduler/status/timestamps, server scan, Tantivy debounce/stale rejection, refresh/error states, and no network work offline.                                   |
| Artists/albums/songs/discovery   | `+page.svelte`; `ColumnBrowser.svelte`; `ArtistGridView.svelte`; `AlbumGridView.svelte`; `ArtistAlbumRail.svelte`; `DetailHeader.svelte`; `SongList.svelte`; `albumList.svelte.ts` | `ui/views/library.rs`; direct `operations/library.rs`; `uniform_list` plus virtual card rows                                | Music columns, Artists, Albums, Recently Added/Played/Most Played, detail/back, multi-disc order, selection, scroll restore, counts/duration/size, loading/empty/error states.               |
| Playlists/offline saving         | `Sidebar.svelte`; `SongList.svelte`; `playlist.svelte.ts`; `contextMenu.ts`; `commands/playlist.rs`                                                                                | `ui/views/playlists.rs`; model selection; `operations/playlist.rs`; typed menus/dialogs                                     | Cached/load/create/rename/delete, add/remove, duplicate-position selection, save/remove offline, reconciliation, cache progress/errors, and offline visibility.                              |
| Transport/queue/shuffle/repeat   | `TransportBar.svelte`; `NowPlayingCenter.svelte`; `QueuePanel.svelte`; playback/queue stores; `commands/{playback,queue}.rs`                                                       | `ui/views/{transport,queue}.rs`; playback watch; `DesktopEvent::QueueChanged`; direct playback/queue operations             | Play/pause/stop, seek, volume, current metadata, add/next/remove/clear/reorder/locate, next/previous, prepared next, shuffle, repeat, reroll, end-of-queue, and persistence.                 |
| Cache/cover art                  | `LazyImage.svelte`; cover-art route; `commands/{cache,coverart}.rs`; `cache/{audio,locations}.rs`                                                                                  | GPUI image/SVG rendering; `ui/views/cover_art.rs`; `operations/{cache,cover_art}.rs`; moved cache modules                   | Thumbnail/full viewer, file paths, original suffix, cache hit/fetch/prefetch/LRU/clear/limit/root move, downloaded IDs, no-overwrite moves, and cache change refresh.                        |
| DSP/normalization/spectrum       | `SpectrumBars.svelte`; `stores/spectrum.svelte.ts`; normalization/playback sections; `commands/{normalization,playback}.rs`; `stereodrome-audio`                                   | `ui/views/{transport,settings}.rs`; spectrum watch; `operations/{normalization,playback}.rs`; unchanged `stereodrome-audio` | EQ, binaural, normalization mode/target/preamp/clipping/dynamics, batch/on-demand analysis, progress/stats/clear, spectrum cadence/idle, gapless/crossfade settings and sound-path behavior. |
| Settings                         | `SettingsModal.svelte`; `commands/settings.rs`; `commands/ui_state.rs`; Tauri store                                                                                                | `ui/views/settings.rs`; typed models; `JsonStore`; direct setting operations/events                                         | Existing section/order, defaults, clamps, enums, units, dependencies, locale/clock formatting, durable values, unknown keys, cancellation, visible errors, and restart restoration.          |
| Last.fm/scrobbling               | Last.fm settings section; `commands/lastfm.rs`; `lastfm.rs`; `credentials.rs`                                                                                                      | `ui/views/settings.rs`; `operations/lastfm.rs`; moved tracker; `App::open_url`; same keyring account                        | Compile-time API inputs, auth begin/browser/complete/disconnect, username/status, now-playing, thresholded scrobble, offline queue/retry, duplicate prevention, and session continuity.      |
| Keyboard/context menus           | `+page.svelte::handleKeydown`; `SongList.svelte`; grid/sidebar context handlers; `services/contextMenu.ts`                                                                         | `ui/actions.rs`; root keybindings; gpui-component context menus                                                             | Every current shortcut, text-input suppression/Escape, row/card/playlist actions, enabled/disabled state, focus restoration, and position-based duplicate actions.                           |
| Notifications                    | `services/notifications.svelte.ts`; `commands/notifications.rs`; Tauri notification plugin/Windows toast                                                                           | `ui/native/notifications.rs` using `notify-rust`; model transition observer                                                 | Now-playing/update notifications, focus and mini-player gates, one notification per transition, title/body, optional image path, text fallback, and visible nonfatal errors.                 |
| Tray/media controls              | `tray/manager.rs`; `media/controls.rs`; `services/{trayControls,mediaControls}.svelte.ts`                                                                                          | `ui/native/{tray,media}.rs`; typed model actions; `tray-icon`; `souvlaki`                                                   | Metadata/art, playback status/position, play/pause/next/previous/seek, tray now-playing/update indicator, Show/Settings/Quit, menu state, HWND wiring, and orderly resource shutdown.        |
| Main/mini/nano/cover-art windows | `tauri.conf.json`; `commands/windowing.rs`; main/mini/cover-art routes                                                                                                             | `ui/windows.rs`; shared `Entity<DesktopModel>`; `WindowKind::Floating`; raw native panel flags                              | 800×600 minimum main; close-to-tray/recreate; 320×72 mini; 30×30 nano; logical saved/clamped coordinates; hover/focus/next-song controls; singleton reusable cover viewer.                   |
| Single instance                  | Tauri single-instance plugin and `src-tauri/src/lib.rs::focus_main_window`                                                                                                         | `ui/native/single_instance.rs` using `single-instance` + `interprocess`; `open_main_window`                                 | One backend/process, second launch messages first, existing main focuses or absent main recreates, startup races handled on every OS.                                                        |
| Updater                          | `stores/updater.svelte.ts`; Tauri updater/process plugins; `tauri.conf.json::plugins.updater`; release workflow                                                                    | `ui/native/updater.rs`; `cargo-packager-updater`; cargo-packager manifests/artifacts                                        | Current/version/notes/check/error/download/install/relaunch, tray and notification indicator, signature/target validation, manual-offline gate, and Tauri→GPUI bridge.                       |
| Shutdown                         | `src-tauri/src/lib.rs::RunEvent::Exit`; client/audio/sync/Last.fm/media/tray workers                                                                                               | `DesktopBackend::shutdown`; `ui/app.rs` ordered quit                                                                        | Reject actions, cancel, stop playback, client shutdown, join all workers, drop native resources, flush stores, close DB/index, exit, and clean restart.                                      |

### Explicit non-port list

These paths have no reachable import/use in the current desktop and must be deleted, not translated:

| Unreachable code                                                       | Disposition                                                                                            |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `src/lib/db/collections.ts`                                            | Delete with its unused TanStack query path.                                                            |
| `src/lib/db/queryClient.ts`                                            | Delete if import verification remains empty after `collections.ts` removal.                            |
| `src/lib/components/library/AlbumGrid.svelte`                          | Delete; reachable grids use `AlbumGridView.svelte`.                                                    |
| `src/lib/components/library/ArtistList.svelte`                         | Delete; reachable artists use `ArtistGridView.svelte`.                                                 |
| `src/lib/stores/nowplaying.svelte.ts`                                  | Delete; no reachable consumer imports it.                                                              |
| `src/lib/components/MarqueeText.svelte`                                | Delete; reachable now-playing text uses `SyncedMarquee.svelte`.                                        |
| `src-tauri/src/commands/nowplaying.rs` and `now-playing` polling event | Delete after confirming the unused store is the only listener; playback metadata remains watch-driven. |

Do not port `@tanstack/db`, `@tanstack/svelte-query`, or query scaffolding solely to preserve these dead paths.

## Data and identity compatibility — release blocker

Compatibility is binary: any mismatch blocks release.

| Contract                        | Value that must remain unchanged                                                               | Proof                                                                                                                |
| ------------------------------- | ---------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| Application/bundle identifier   | `dev.xikxp1.stereodrome`                                                                       | Installed Tauri and GPUI builds resolve the same proven profile and OS identity.                                     |
| Data directory                  | Tauri’s existing per-OS app-data directory for that identifier                                 | Compare paths before DB open; run copied-profile startup on every OS.                                                |
| Database                        | `stereodrome.db`                                                                               | Record schema/version/table counts and representative rows before/after both launches.                               |
| Canonical schema                | Exact current `src-tauri/src/db/schema.sql`, moved to `crates/stereodrome-core/src/schema.sql` | Byte hash is identical; both consumers use `DESKTOP_SCHEMA`.                                                         |
| Desktop migrations              | Existing `src-tauri/src/db/mod.rs::run_migrations` logic/order                                 | Move without weakening; run old and fresh fixtures through it.                                                       |
| Search index                    | `search_index` directory and current Tantivy schema/version                                    | Query representative artist/album/song terms before and after; no rebuild unless existing code already requires one. |
| Settings file                   | `settings.json`                                                                                | All known and injected unknown keys/values survive writes and restart.                                               |
| Runtime state file              | `state.json`                                                                                   | `volume` and `mini_player_position` survive restart unchanged except existing clamps.                                |
| Default caches                  | Existing profile’s `audio_cache` and `cover_cache`                                             | Existing files are hit without refetch; paths and byte counts match.                                                 |
| Custom cache root               | Absolute `cache_root` value in `settings.json`                                                 | GPUI opens it in place; no copy/move is triggered at startup.                                                        |
| Keyring service                 | `stereodrome`                                                                                  | Read the same OS keyring entries from installed Tauri and GPUI builds.                                               |
| Server keyring account/payload  | `server_credentials` with JSON `url`, `username`, `password`                                   | Restore succeeds without prompting and without rewriting on read.                                                    |
| Last.fm keyring account/payload | `lastfm_session` with JSON `username`, `session_key`                                           | Status/authenticated username restores without reauthorization.                                                      |

### Schema move rules

- Move the SQL before `src-tauri` can be deleted.
- Do not edit SQL while moving it. Schema improvements are a separate migration after the UI cutover.
- Export `DESKTOP_SCHEMA` from `stereodrome-core`; update its current relative include and the desktop migration runner to consume that one constant.
- Keep the desktop migration runner because `StereodromeCore`’s mobile migration path is not a replacement for desktop history.
- Test fresh, last released, and representative long-lived copied databases. A failed migration aborts startup and leaves the original fixture available for comparison.

### Copied-profile release fixture

For each OS, make a disposable filesystem copy of an existing profile and use test-only duplicated keyring entries under the **same service/account names in an isolated OS user/VM**. Record before/after:

1. canonical/actual data path;
2. SHA-256 and size for both JSON files plus unknown sentinel keys;
3. SQLite integrity check, schema objects, migration state, table counts, queue state, and representative playlist duplicate positions;
4. Tantivy directory contents and representative query results;
5. cache root, file counts/sizes, and a known cached playback/cover-art hit;
6. restored server URL/username and Last.fm username without logging secret values;
7. volume and mini-player coordinates;
8. state after Tauri start/quit, GPUI start/quit, and second GPUI restart.

The GPUI run may perform normal timestamp/LRU/queue/playback writes caused by the walkthrough; compare semantic state and explicitly account for those fields. Any new profile directory, credential prompt, empty library/index, cache refetch, lost unknown key, SQL error, or reset state fails the gate.

## Native replacement choices

Library selection is closed. Add no wrapper framework; each small native module owns one concrete library and dispatches typed model actions.

| Responsibility         | Choice                                               | Required integration                                                                                                                                                                                                                                                                                                                |
| ---------------------- | ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Media controls         | Keep `souvlaki` `0.8.3`                              | Move current metadata/playback/seek command thread. On Windows pass the main GPUI HWND obtained through raw-window-handle; use the platform’s normal no-HWND config elsewhere. Convert callbacks to Play/Pause/Toggle/Next/Previous/Seek model actions.                                                                             |
| Tray                   | `tray-icon` `0.24.1`                                 | Create the icon on the thread running the required native event loop: GPUI/main thread on macOS; the same Win32/GTK event-loop thread on Windows/Linux. Use its event handlers to wake GPUI, then dispatch Show/Settings/Play/Pause/Next/Previous/Quit. Retain existing Linux GTK/appindicator packages.                            |
| Notifications          | `notify-rust` `4.18.0`                               | Send now-playing and update notifications on macOS, Windows, and Linux. Pass an existing cover-art file path when available; otherwise send text. Notification failure is visible/logged but never interrupts playback.                                                                                                             |
| Single instance        | `single-instance` `0.3.3` + `interprocess` `2.4.2`   | Acquire the per-user `dev.xikxp1.stereodrome` instance lock before backend open. First instance starts a local listener; second connects, sends a bounded typed `ShowMain` message, and exits. The receiver wakes GPUI and calls `open_main_window`. Retry briefly for the lock-won/listener-start race; reject malformed messages. |
| URL open               | GPUI `App::open_url`                                 | Last.fm authorization, homepage/release links. Surface failure. No opener plugin or extra crate.                                                                                                                                                                                                                                    |
| Folder selection       | GPUI `App::prompt_for_paths`                         | Request `{ files: false, directories: true, multiple: false }`. `None` is cancellation/no change; error is visible. Linux-only `rfd` `0.17.2` fallback is allowed only after the Phase 0 native picker gate fails with required portal packages installed.                                                                          |
| Main/secondary windows | GPUI window API                                      | Main uses an 800×600 minimum. Mini/nano use `WindowKind::Floating`. Closed main/mini windows are removed and later recreated from the shared model because GPUI has no public per-window hide API. Cover viewer is a singleton window whose model input can change.                                                                 |
| macOS panel flags      | raw-window-handle + existing `objc2`/AppKit versions | After window creation, apply nonactivating, floating, all-spaces/full-screen auxiliary behavior missing from public GPUI. Keep this code `cfg(target_os = "macos")`; do not fork GPUI unless the pinned source cannot expose a native handle.                                                                                       |
| Packaging              | `cargo-packager` `0.11.8`                            | DMG/app, NSIS/MSI, AppImage/DEB, icons/metadata, and signed updater artifacts. Preserve current identity/descriptions/artifact names.                                                                                                                                                                                               |
| Updater                | `cargo-packager-updater` `0.2.3`                     | Check signed target/version metadata, expose progress/error to the model, install, and relaunch only after success. Keep separate Tauri/GPUI manifests during bridge releases.                                                                                                                                                      |
| RPM                    | `cargo-generate-rpm` `0.21.0`                        | Generate the existing RPM artifact separately because cargo-packager does not generate RPM. Share binary, icons, desktop entry, descriptions, version, and dependencies with the other Linux packages.                                                                                                                              |

### Threading and action ownership

- GPUI entities/windows are touched only on the GPUI thread.
- Native callbacks send a small typed shell action through the proven wake-up path. They never lock `DesktopState` or call a GPUI entity directly.
- `souvlaki` and tray resources retain command senders plus join/shutdown handles. Drop is a fallback; ordered quit explicitly shuts them down.
- The tray is created before allowing the last visible window to close. If tray initialization fails, disable close-to-tray and make main close quit visibly/safely rather than leaving a headless process.
- Media/tray state is a projection of `DesktopModel`; it is never authoritative.

### Single-instance protocol

Use a fixed, versioned message with no arbitrary path or command execution:

```rust
enum InstanceMessage {
    V1ShowMain,
}
```

Scope the IPC endpoint to the current OS user and application identifier, set user-only permissions where supported, cap message size, read one message, acknowledge, and close. Backend/profile open occurs only in the lock-owning first instance.

### Window lifecycle

- `open_main_window` checks the stored handle, activates/focuses it when present, or creates a new root around the shared model when absent.
- Main close clears its handle and removes the window unless `quitting`; it does not stop playback/backend/native services.
- Mini and nano are two modes of one secondary-window slot. Switching mode recreates/resizes at a monitor-clamped logical position and preserves model state.
- Cover-art actions update the singleton cover-view model before focusing/creating its window.
- Explicit Quit is the only path that tears down the backend.

## Primary references

GPUI and gpui-component are pre-1.0, actively developed, and currently learned primarily from pinned source and examples. Re-check these exact sources when changing a pin; do not assume current website examples match the locked revisions.

### GPUI at `1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba`

- [GPUI site](https://gpui.rs/)
- [Pinned GPUI README](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/README.md)
- [Pinned Zed Rust toolchain](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/rust-toolchain.toml)
- [Contexts guide](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/docs/contexts.md)
- [Key dispatch guide](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/docs/key_dispatch.md)
- [Window example](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/examples/window.rs)
- [Application menu example](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/examples/set_menus.rs)
- [Text/IME input example](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/examples/input.rs)
- [`uniform_list` example](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/examples/uniform_list.rs)
- [Accessibility example](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/examples/a11y.rs)
- [Image example](https://github.com/zed-industries/zed/tree/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/examples/image)
- [Window implementation](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/src/window.rs)
- [Platform interface](https://github.com/zed-industries/zed/blob/1a246efd7e1b83ab568ec5e3e6c1a43a42e1abba/crates/gpui/src/platform.rs)

### gpui-component at `031555662e99a1b5a549990b47f246d475b8288a`

- [Pinned README and component inventory](https://github.com/longbridge/gpui-component/blob/031555662e99a1b5a549990b47f246d475b8288a/README.md)
- [Pinned component gallery source](https://github.com/longbridge/gpui-component/tree/031555662e99a1b5a549990b47f246d475b8288a/crates/story)
- [Pinned lockfile showing the matching Zed source revision](https://github.com/longbridge/gpui-component/blob/031555662e99a1b5a549990b47f246d475b8288a/Cargo.lock)

### Packaging and native services

- [cargo-packager README](https://github.com/crabnebula-dev/cargo-packager/blob/main/README.md)
- [cargo-packager-updater README](https://github.com/crabnebula-dev/cargo-packager/blob/main/crates/updater/README.md)
- [cargo-generate-rpm README](https://github.com/cat-in-136/cargo-generate-rpm/blob/master/README.md)
- [tray-icon README and platform event-loop requirements](https://github.com/tauri-apps/tray-icon/blob/dev/README.md)
- [souvlaki](https://github.com/Sinono3/souvlaki)
- [notify-rust](https://github.com/hoodie/notify-rust)

## Verification

Run commands from the repository root unless a subshell is shown. Record command, commit, OS/architecture, and result. Narrow checks may diagnose a failure but do not replace the gates below.

### Phase 0 native feasibility

On native macOS, Windows, and Linux hosts:

```sh
cargo fmt --all --check
cargo clippy -p stereodrome-desktop --all-targets --features gpui-ui -- -D warnings
cargo test -p stereodrome-desktop --no-default-features
cargo build -p stereodrome-desktop --features gpui-ui --bin stereodrome
cargo build -p stereodrome-desktop --release --features gpui-ui --bin stereodrome
cargo run -p stereodrome-desktop --features gpui-ui --bin stereodrome
```

The `cargo run` result is complete only after the Phase 0 text/IME, AccessKit, image/SVG, list/grid virtualization, menu/context menu, folder picker, Tokio delivery, windows, raw handle/Windows media, tray wake-up, Linux runtime, and macOS panel checklist is recorded.

### Every coexistence change

Keep the shipping shell green while Tauri exists:

```sh
bun run check
bun run lint
bun run format:check
cargo fmt --all --check
cargo clippy -p stereodrome -- -D warnings
cargo test -p stereodrome
cargo build -p stereodrome
cargo clippy -p stereodrome-desktop --all-targets --no-default-features -- -D warnings
cargo clippy -p stereodrome-desktop --all-targets --features gpui-ui -- -D warnings
cargo test -p stereodrome-desktop --no-default-features
cargo build -p stereodrome-desktop --features gpui-ui --bin stereodrome
cargo run -p stereodrome-desktop --features gpui-ui --bin stereodrome
```

Run the Tauri app and exercise the moved path as the smoke test; a wrapper compile is not proof that the production shell still behaves.

### Schema/shared/mobile gate

After moving the schema or changing shared crates:

```sh
cargo fmt --all --check
cargo clippy -p stereodrome-audio -p stereodrome-core -p stereodrome-ffi -- -D warnings
cargo test -p stereodrome-audio -p stereodrome-core -p stereodrome-ffi
(cd mobile && bun run typecheck)
(cd mobile && bun run lint)
(cd mobile && bun run rust:check)
```

Run `bun run rust:check` on a host provisioned with both Xcode/iOS Rust targets and Android SDK/NDK/Rust targets so `all` checks both native bridges. The mobile TypeScript wrapper and `StereodromeCore`/FFI operation contract must have no migration-driven diff except relocated icon paths in Phase 7.

### Phase 2 backend gate

In addition to coexistence checks:

```sh
cargo tree -p stereodrome-desktop -d
cargo build -p stereodrome-desktop --no-default-features
cargo test -p stereodrome-desktop --no-default-features
```

Inspect the lockfile/source tree: one Zed revision only, no Tauri dependency in the backend-only graph, and no `tauri`, `AppHandle`, or `State` import in backend modules. Run backend behavior checks against fresh and copied databases before the Tauri parity smoke run.

### Phases 3–6 GPUI behavior gate

For each accepted vertical slice, run the debug binary and the relevant portion of the manual walkthrough on all three OSes. Also run a release binary once per phase to catch feature/resource differences:

```sh
cargo build -p stereodrome-desktop --release --features gpui-ui --bin stereodrome
```

Use platform screen readers (VoiceOver, Narrator, and Orca), native keyboard navigation, X11 and Wayland on Linux, multiple monitor scales, network disconnect/manual offline, and a library large enough to expose non-virtualized rendering.

### Phase 7 package/update gate

CI must build and retain:

- macOS universal app/DMG plus signed update;
- Windows MSI, NSIS EXE, and signed update;
- Linux AppImage, DEB, RPM, and signed update;
- the final Tauri artifacts during coexistence;
- unchanged Android APK/AAB and iOS IPA jobs.

Install each package into a clean VM/user and a copied-profile VM/user. Record signature/notarization/package verification, launch, desktop/menu entry, icons, audio, tray, picker, updater, uninstall behavior, and profile retention. Then run the signed update chain described below.

### Phase 8 final automated gate

After Tauri/web deletion:

```sh
cargo fmt --all --check
cargo clippy -p stereodrome-desktop -p stereodrome-audio -p stereodrome-core -p stereodrome-ffi --all-targets -- -D warnings
cargo test -p stereodrome-desktop -p stereodrome-audio -p stereodrome-core -p stereodrome-ffi
cargo build -p stereodrome-desktop --release --bin stereodrome
(cd mobile && bun run typecheck)
(cd mobile && bun run lint)
(cd mobile && bun run rust:check)
```

These commands assume Phase 8 has removed the transitional `gpui-ui` feature as required. Repository search must show no runtime/build Tauri/web dependency.

## Full parity walkthrough

### Fixture

Use a disposable copy of an existing desktop profile plus a Subsonic-compatible test account containing:

- multiple artists and albums, cover art, and at least one multi-disc album;
- tracks longer than 30 seconds and consecutive same-album tracks for gapless testing;
- enough mixed-album songs to observe shuffle and reroll;
- a playlist containing the same song at two distinct positions;
- cached and uncached audio/art plus a writable custom cache root;
- a Last.fm test account and queued scrobble fixture;
- a second monitor or simulated monitor topology;
- signed last-Tauri, first-GPUI, and next-GPUI test releases.

Inject an unknown sentinel key into both JSON files. Capture the compatibility baseline before step 1.

### Walkthrough and expected observations

| Step | Action                                                                                                                                              | Expected observable state                                                                                                                                                                                                                               |
| ---- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1    | Install/launch with a fresh profile.                                                                                                                | One main window opens at 800×600 or larger and cannot shrink below 800×600; login is focused/named; the proven legacy directory is the only profile created.                                                                                            |
| 2    | Connect with invalid then valid server credentials.                                                                                                 | Invalid input/network/auth shows a non-destructive error; valid connect stores the same keyring payload, shows configured server/user/version, loads the library, and does not expose the password.                                                     |
| 3    | Quit and relaunch online, then relaunch with network unavailable.                                                                                   | Online restores without a prompt; unavailable network retains configured identity, enters the existing offline-capable state, loads local data, and does not clear credentials/library.                                                                 |
| 4    | Enable manual Offline mode, try sync/scan/recent/Last.fm/update, then disable it.                                                                   | Network-only actions are disabled or visibly rejected without requests; cached library/playback remains available; disabling restores connection/status and network actions.                                                                            |
| 5    | Visit Music and select genre → artist → album; use keyboard up/down/Enter and artist/album links.                                                   | All columns derive from visible songs; song order includes disc/track ordering; selection/focus/playing/downloaded state and navigation match Tauri.                                                                                                    |
| 6    | Visit Artists and Albums; scroll deeply, open artist/album detail, traverse artist album rail and back.                                             | Only visible virtual rows render; scroll offsets restore; detail headers/counts/art/songs and nested back target are correct.                                                                                                                           |
| 7    | Visit Recently Added, Recently Played, and Most Played; open album/artist links.                                                                    | Server ordering/count/loading/error states match; navigation resolves local detail; offline mode sends no discovery request.                                                                                                                            |
| 8    | Type search queries rapidly, clear during an in-flight search, and switch account/disconnect.                                                       | Debounce reduces calls; only newest same-account results appear; clear/account change invalidates stale results; controls are gated only while the accepted query is pending.                                                                           |
| 9    | Trigger incremental sync, full reconcile, and server scan; wait for scheduler/status changes.                                                       | Progress/job kind/timestamps/counts/errors update through typed events; new content refreshes views/index; overlapping jobs remain prevented; restart retains schedule state.                                                                           |
| 10   | Create, rename, select, add to, and delete a playlist through buttons/context menus/dialogs.                                                        | Counts/content update once; cancel mutates nothing; destructive focus returns predictably; keyboard and screen reader state are correct.                                                                                                                |
| 11   | Remove only one of two duplicate song positions; save playlist offline, interrupt/retry, then remove offline.                                       | The chosen occurrence alone disappears; cache progress/errors/reconciliation are accurate; downloaded markers/offline visibility update; removal follows existing cache ownership rules.                                                                |
| 12   | Open uncached then cached cover art/audio; open/update/close/reopen cover viewer; change cache limit/root and reset it.                             | First access fetches and preserves suffix, second hits disk; viewer is singleton and updates/focuses; LRU/limits/move-no-overwrite summaries/paths match; picker cancel changes nothing and failure is visible.                                         |
| 13   | Start a track; play/pause, seek boundaries, change volume, stop/resume, and restart the app.                                                        | Playback watch, transport, OS metadata/position, seek clamps, finite volume, persisted volume, current art, and stopped/playing states remain synchronized.                                                                                             |
| 14   | Build a queue; add/insert next/remove/clear/reorder/locate current; next/previous; toggle shuffle/repeat; reroll.                                   | Queue projection/persistence/current/prepared-next update once per action; reorder and locate are stable; shuffle/repeat/reroll match existing queue semantics after restart.                                                                           |
| 15   | Play consecutive same-album tracks, mixed-album tracks with crossfade, manual Next crossfade, and reach queue end.                                  | Eligible tracks are gapless; configured crossfade duration applies only to existing eligible paths; no double advance/scrobble; queue/playback ended states are emitted once.                                                                           |
| 16   | Toggle EQ bands, binaural presets, normalization track/album/target/preamp/clipping, dynamics presets, and spectrum; analyze/clear normalization.   | Audio settings clamp/persist/reapply; progress/stats/clear states are accurate; spectrum updates/returns idle and disabling stops visual repaint; no UI-side DSP implementation appears.                                                                |
| 17   | Authenticate/disconnect Last.fm; play beyond the current scrobble threshold; go offline, queue, reconnect/retry.                                    | Browser/auth status and keyring session restore; now-playing/scrobble occurs once; queued items/status/retry match existing duplicate and threshold rules.                                                                                              |
| 18   | Change every Settings control in section order, cancel/confirm each destructive dialog, quit/relaunch.                                              | Updates, Server/scan, Library Sync, Display, Notifications, Last.fm, Playback/EQ, Normalization/dynamics, and Cache show persisted/clamped values, correct dependencies, errors, locale/time, and restored dialog focus; unknown JSON sentinels remain. |
| 19   | Exercise Space, Enter, modifier/Shift arrows, modifier volume, M/S/R/Q/V/D, modifier+K, modifier+Comma, and every row/card/playlist context action. | Actions match Tauri; typing consumes shortcuts; Escape restores input focus behavior; menu enabled/selected state and duplicate-position actions are correct in component menus.                                                                        |
| 20   | Enable notification permutations; change track with main focused/unfocused and mini open; expose/miss cover path.                                   | Exactly the allowed transitions notify with title/body/image when present and text fallback when absent; notification failure is nonfatal and visible.                                                                                                  |
| 21   | Use OS media play/pause/seek/next/previous and every tray item, including Settings, Show, update indicator, and Quit.                               | Backend/model/transport stay authoritative and synchronized; tray labels/metadata/art/status update; callbacks wake GPUI; Quit follows ordered shutdown.                                                                                                |
| 22   | Open mini, switch 320×72 ↔ 30×30 nano, move across scaled monitors, disconnect a monitor, hover/focus, close/reopen; reuse cover viewer.            | Size/mode/controls/next-song setting match; logical position persists and clamps to available work area; macOS stays nonactivating/all-spaces; shared playback state is immediate after recreation.                                                     |
| 23   | Close main during playback, invoke tray Show, then launch a second instance with main open and closed.                                              | Backend/playback/queue continue headless; Show/second launch focuses or recreates one main window; no second backend, worker set, DB opener, or event subscription appears.                                                                             |
| 24   | Quit during active playback/sync and relaunch.                                                                                                      | New actions stop; schedulers/monitors cancel; audio/client stop; all retained workers join; stores/DB/index reopen without lock, corruption, duplicate queue advance, or lost state.                                                                    |
| 25   | Install every clean package and uninstall without selecting data removal.                                                                           | Correct identity, icons, desktop/start menu entries, audio/tray/picker/native services; uninstall removes app files but leaves profile/keyring data.                                                                                                    |
| 26   | Install last Tauri, populate/copy state, update to first GPUI, then update to next GPUI.                                                            | Each signed update validates/launches; identifier/profile/keyring/database/index/cache/settings/queue remain continuous; incompatible OS uses only the documented signed-installer bridge.                                                              |

Complete the compatibility capture again after step 26 and attach differences with explanations. Any unexplained parity or persistence difference blocks Phase 8.
