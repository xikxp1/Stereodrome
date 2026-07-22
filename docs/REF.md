# Lean Mobile / Rust Runtime Refactor Proposal

Status: proposal, investigated against commit `12436c9`.

This document proposes replacing the mobile client's distributed orchestration with one Rust-owned operational runtime. The goal is not merely fewer React components or a smaller bridge. The goal is to make every Stereodrome operation pass through one serialized, observable, recoverable state machine so queue, playback, connectivity, sync, downloads, persistence, and background work cannot independently disagree.

`docs/MOBILE_PLAYBACK_SYNC.md` solved an important part of the earlier playback problem: Rust now emits sequenced playback snapshots and native code projects them directly to platform media sessions. This proposal preserves that work and extends its single-source-of-truth principle to the rest of the application.

## Executive summary

The current implementation has robust pieces, but they are composed as several partial state machines:

- React owns lifecycle reconciliation, action locks, playback restoration, network policy, sync completion handling, and query invalidation.
- `stereodrome-ffi` owns live audio orchestration, job state, prefetch state, event sequencing, playback monitoring, and additional Tokio runtimes/threads.
- `stereodrome-core` owns durable data, queue rules, cache policy, server operations, and playback/scrobble persistence, but not the complete live operational state.
- Swift and Kotlin own platform session/focus state and each contain another serialized command path.
- Desktop still has its own `AppState` and duplicates orchestration around the shared queue/audio crates.

The result is defensive complexity: generation counters, sequence filters, local action locks, pending-refresh flags, duplicate status reads, method-name allowlists, and platform-specific recovery branches. These mechanisms are individually reasonable, but their quantity is evidence that there is no single transition boundary.

The recommended target is:

1. A single long-lived `StereodromeRuntime` in Rust per process/data directory.
2. One serialized command mailbox for every mutation and platform/lifecycle input.
3. One authoritative `CoreState` with explicit domain states and monotonic revision.
4. Long-running work represented as state-machine effects with operation IDs, cancellation, and stale-completion rejection.
5. A thin FFI protocol: initialize, dispatch a typed command, read a snapshot, subscribe to one event stream, destroy.
6. Native modules that only own unavoidable platform resources and translate platform events/projections.
7. React Native as a renderer and intent sender, not an operational coordinator.

Do this incrementally. A big-bang rewrite would recreate the same failure modes without characterization tests.

## Investigation scope

The audit covered:

- React orchestration and bridge client:
  - `mobile/src/context/PlaybackContext.tsx`
  - `mobile/src/context/StereodromeContext.tsx`
  - `mobile/src/services/stereodromeCore.ts`
  - `mobile/src/services/librarySyncScheduler.ts`
  - direct core calls from mobile screens
- Native adapters:
  - `mobile/modules/stereodrome-core/ios/StereodromeCoreModule.swift`
  - `mobile/modules/stereodrome-core/android/src/main/java/expo/modules/stereodromecore/*`
  - `mobile/modules/stereodrome-core/android/src/main/cpp/stereodrome_core_jni.cpp`
- Shared/mobile Rust:
  - `crates/stereodrome-core`
  - `crates/stereodrome-ffi`
  - `crates/stereodrome-audio`
- Desktop seams:
  - `src-tauri/src/state.rs`
  - `src-tauri/src/commands/queue.rs`
  - `src-tauri/src/commands/playback.rs`
  - `src-tauri/src/audio/player.rs`
- Recent mobile stabilization history, especially commits concerning React/scheduler churn, failure recovery, prefetch cancellation, native ownership, projection deduplication, seek accumulation, and stale queue state.

## Current architecture

```text
React screens
  ├─ TanStack Query: library/read models
  ├─ StereodromeContext: connection/offline/sync/file-state lifecycle
  └─ PlaybackContext: live playback mirror, restoration, action locks, seek batching
             │
             ▼
mobile/src/services/stereodromeCore.ts
  ├─ 1,100+ lines of wrappers and hand-written payload validators
  ├─ string method names + JSON envelopes
  └─ playback callback + generic core-event callback
             │
             ▼
Swift / Kotlin / JNI
  ├─ core lifetime and command serialization
  ├─ audio-session/focus handling
  ├─ media-session projection
  └─ remote-control command paths
             │
             ▼
crates/stereodrome-ffi/src/lib.rs
  ├─ 3,500+ lines and about 88 dispatch arms
  ├─ AudioPlayer + StereodromeCore composition
  ├─ playback monitor and snapshot announcer
  ├─ sync/offline/prefetch job state
  ├─ cancellation and method-name side-effect policies
  └─ runtime/thread creation
             │
             ├──────────► crates/stereodrome-audio
             ▼
crates/stereodrome-core/src/lib.rs
  ├─ 5,700+ line service object
  ├─ SQLite, server client, queue, cache, sync, playlists, Last.fm
  └─ durable playback/settings state
```

Rust is already the dominant implementation layer. The bloat comes primarily from where the pieces are composed and where state ownership remains split.

## Findings

### F1. A user action is often a multi-call distributed transaction

`PlaybackContext` performs operations such as:

- play: `playSongWithQueue` → `audioPlayCurrent` → snapshot reconciliation → optional seek → another reconciliation → prepare-next;
- clear: `clearQueue` → `audioStop`;
- startup restore: fetch live snapshot and persisted playback state → potentially mutate queue → apply settings → rebuild playback → reconcile again;
- settings change: persist settings → reapply live audio → prepare transition.

Each step can succeed while the next fails, be interleaved with a native remote command, or become stale after an autonomous Rust transition. React's `actionLocksRef`, seek drain, refs, and sequence checks reduce individual races but cannot make the whole operation atomic.

**Required change:** expose intent-level commands such as `PlaySelection`, `ClearPlayback`, `Resume`, `ApplyAudioSettings`, and `SeekBy`. Rust must own each complete transition.

### F2. There are multiple independent serializers, none of which covers all inputs

Current serialization includes:

- React `actionLocksRef` for selected UI actions;
- React's seek queue;
- Swift `coreQueue` and `remoteCommandQueue`;
- Android's synchronized bridge plus `StereodromeCoreCommandQueue`;
- Rust mutexes around queue, sync, saved-playlist work, prefetch, event sequences, and file state;
- the audio engine's own command path.

Native controls do not participate in React locks, background workers do not participate in native command queues, and the individual Rust mutexes do not establish one global transition order.

**Required change:** every mutating input, regardless of origin, enters one Rust mailbox. Platform queues may be needed to avoid blocking OS callbacks, but they must only enqueue an intent and never implement operation policy.

### F3. Live state is partitioned across `stereodrome-core` and `stereodrome-ffi`

Examples:

- `StereodromeCore::get_library_sync_status` always constructs `active_job: None`; `stereodrome-ffi` overlays `MobileSyncState` to produce the actual mobile status.
- Durable playback position is in core/SQLite, live playback is in `AudioPlayer`, and their combined snapshot is built in FFI.
- Download truth is split between SQLite/filesystem, process-global download maps, and FFI's `MobileFileStateSnapshot`.
- Queue prefetch policy is implemented partly in core and partly by `BackgroundPrefetchState` in FFI.

The crate named FFI is therefore an application runtime, not an adapter. Its behavior is not directly available to desktop and is difficult to test without crossing several layers.

**Required change:** move runtime composition, job ownership, monitoring, event generation, and transition policy into `stereodrome-core` (or a `runtime` module exported by it). Leave C ABI concerns in `stereodrome-ffi`.

### F4. Background jobs create additional core instances and runtimes

Mobile sync and saved-playlist jobs spawn threads, create a new `StereodromeCore` for the same data directory, create another Tokio runtime, restore another server session, and operate against the same database/filesystem while the primary core remains alive. The playback monitor also creates its own runtime.

Locks protecting fields on one core instance do not coordinate fields on another instance. Some safety currently comes from SQLite transactions and process-global download locks, but ownership is implicit and operation-specific.

**Required change:** one runtime owns one repository/client/cache/job registry. Background work runs as tasks owned by that runtime and reports completion back to its mailbox.

### F5. String dispatch requires synchronized manual policy lists

`stereodrome-ffi::dispatch` matches string method names. Separate functions decide which method names cancel prefetch and which emit playback snapshots. TypeScript repeats method names and response validators. Swift and Kotlin repeat playback-affecting method lists for audio-session/focus acquisition.

Adding or renaming an operation can compile while one side-effect list remains stale. This is a direct point of failure, not merely a style concern.

**Required change:** deserialize a tagged `CoreCommand` enum. Side effects must follow from command handling/state transitions, not from method-name classification. Generate or test the TypeScript protocol from Rust definitions.

### F6. Lifecycle and connectivity policy are duplicated above Rust

At least four app-state subscriptions currently influence data or playback behavior across `App.tsx`, `StereodromeContext`, and `PlaybackContext`. `StereodromeContext` also owns network generations, pending foreground refreshes, offline derivation, sync completion detection, and saved-playlist reconciliation. The background task repeats manual-offline and connection checks before asking Rust to run a due sync, while Rust checks offline mode and due-job policy again.

React must still tell Rust about app/network lifecycle changes, and Expo must still register an OS background task. It should not decide what those changes mean for core operations.

**Required change:** native/JS report facts (`Foregrounded`, `Backgrounded`, `NetworkChanged`, `BackgroundTick`). Rust decides whether to restore a connection, reconcile, sync, cancel, or remain offline.

### F7. The bridge contract is larger and more defensive than necessary

`mobile/src/services/stereodromeCore.ts` is mostly hand-written runtime validators and one wrapper per FFI method. This catches protocol drift, but it also duplicates all serialized Rust models and makes every new field touch several layers.

There are currently two event paths and sequence domains: playback snapshots and generic core events. Both ultimately originate from the same runtime.

**Required change:** use one versioned command/event protocol and one event callback. Retain runtime validation at the envelope/version boundary; generate payload types and protocol fixtures rather than hand-maintaining hundreds of field checks.

### F8. Native platform adapters correctly own resources, but also reconstruct policy

Swift and Kotlin must own audio-session/focus APIs, media sessions, services, interruption callbacks, and OS threading rules. That is unavoidable. They should not decide whether a core can play, query live snapshots to implement toggle policy, optimistically mutate projected playback state, or maintain independent command semantics.

The manual JSON projection parsers in Swift and Kotlin also duplicate playback field interpretation.

**Required change:** Rust emits a `PlatformProjection` containing metadata, capabilities, and desired presentation. Native maps it to platform APIs. Native reports platform facts back as typed events. It does not infer core transitions.

### F9. Desktop remains a second orchestration implementation

`src-tauri::AppState` separately owns the client, database connection, audio player, queue, navigation guard, Last.fm tracker, and analysis progress. Desktop queue/playback commands duplicate composition already performed for mobile in FFI.

A mobile-only runtime refactor would improve mobile but leave behavior drift and duplicated fixes.

**Required change:** design the runtime API to be platform-neutral from the start, then migrate desktop after the mobile path proves it. Tauri commands/events become another adapter over the same runtime.

### F10. Error handling is string-based and operational recovery is not stateful

Core errors become strings at FFI. React generally stores one last error string. Native command queues often log errors after acknowledging an OS command. Jobs carry limited running/last-error state but no common operation identity, retry policy, cancellation state, or failure classification.

**Required change:** errors need stable codes, domain, retryability, operation ID, and context. Failure must transition state (for example `Playback::Failed { recoverable: true }`), not only produce a rejected promise or log line.

## What is already robust and should be preserved

The refactor should reuse, not discard, the strongest current work:

- `PlayQueue` has extensive repeat/shuffle/navigation tests and durable original-order handling.
- Queue prepare/commit checks such as `play_next_if_matches` guard asynchronous preparation.
- SQLite writes use transactions in queue, sync, and backup paths.
- Download deduplication, permits, cancellation tokens, prefetch generations, retry/backoff, and cooldown logic are valuable.
- Playback snapshots are sequenced, atomically captured, and projected to native before returning through callbacks.
- The audio monitor handles autonomous gapless, crossfade, progress, end-of-track, and cache events without relying on JS.
- FFI catches Rust panics at the ABI boundary.
- Native media controls already work without requiring React to remain awake.
- Core/FFI queue, cache, backup, and event behavior have meaningful Rust test coverage.

These become services/effects behind the state machine rather than being rewritten wholesale.

## Target architecture

### Ownership rule

> If a decision changes Stereodrome's durable or operational behavior, Rust owns it. If a decision is required solely to interact with an OS API or render UI, the platform adapter owns it.

Examples:

| Concern                                                                 | Owner                                      |
| ----------------------------------------------------------------------- | ------------------------------------------ |
| Queue, current item, repeat/shuffle, transition intent                  | Rust runtime                               |
| Playback phase, restore policy, seek coalescing, next preparation       | Rust runtime                               |
| Connection/offline policy, sync scheduling decision                     | Rust runtime                               |
| Download/offline jobs, cancellation, retries                            | Rust runtime                               |
| Persistence and migrations                                              | Rust core repositories                     |
| Audio decoding/DSP/output state                                         | `stereodrome-audio`, controlled by runtime |
| iOS audio-session activation and interruption callbacks                 | Swift adapter                              |
| Android audio focus, Media3 service/session                             | Kotlin adapter                             |
| Background-task registration constraints                                | Expo/native adapter                        |
| Library list rendering, navigation, selection, local visual preferences | React Native                               |

### Runtime topology

```text
React / Tauri / native remote controls / OS lifecycle
                         │ CoreCommand
                         ▼
                StereodromeRuntimeHandle
                         │ bounded mailbox
                         ▼
┌─────────────────────────────────────────────────────────────┐
│ Single runtime actor                                        │
│                                                             │
│ CoreState { revision, lifecycle, connectivity, playback,    │
│             queue, jobs, downloads, settings, errors }      │
│                                                             │
│ reduce(command/event)                                       │
│   ├─ validate transition and assign operation_id            │
│   ├─ commit immediate state                                 │
│   ├─ persist required intent/state                          │
│   ├─ emit CoreEvent                                         │
│   └─ start/cancel Effect                                    │
│                         ▲                                   │
│ async effects ─────────┘ EffectCompleted(operation_id, ...) │
└─────────────────────────────────────────────────────────────┘
         │              │               │
         ▼              ▼               ▼
 repositories       server client    AudioPort
 (SQLite/files)     / sync/cache      (`stereodrome-audio`)
```

Long operations must not block the actor. The actor records an operation as running, starts an async effect, and processes other commands. Completion returns as an internal event with an operation ID/generation. A stale completion is ignored by construction.

### Proposed Rust modules

Do not put the new design into another monolithic `lib.rs`.

```text
crates/stereodrome-core/src/
  lib.rs                     # public exports only
  protocol.rs                # versioned command/result/event types
  runtime/
    mod.rs                   # handle, actor startup/shutdown
    state.rs                 # CoreState and domain states
    reducer.rs               # transition validation and effect creation
    effect.rs                # internal effect/completion types
    snapshot.rs              # consumer-facing snapshots/projections
  services/
    connection.rs
    library.rs
    playback.rs
    downloads.rs
    playlists.rs
    scrobble.rs
    backup.rs
  repository/
    mod.rs
    migrations.rs
    queue.rs
    settings.rs
    playback.rs
```

This is a target shape, not a requirement to move every existing function before useful runtime work begins.

### State model

`CoreState` is operational state, not an in-memory copy of the full library. Library rows remain queryable read models in SQLite.

```rust
pub struct CoreState {
    pub revision: u64,
    pub lifecycle: RuntimeLifecycle,
    pub connectivity: ConnectivityState,
    pub playback: PlaybackState,
    pub queue: QueueState,
    pub sync: SyncState,
    pub downloads: DownloadState,
    pub settings_revision: u64,
    pub library_revision: u64,
    pub last_failure: Option<OperationFailure>,
}
```

Recommended explicit states include:

- runtime: `Starting | Ready | ShuttingDown | Faulted`;
- connectivity: `Unconfigured | OfflineManual | Disconnected | Connecting | Online | Degraded`;
- playback: `Empty | Preparing | Playing | Paused | Stalled | Stopped | Failed`;
- jobs: `Idle | Running { operation_id, kind, progress } | Cancelling | Failed`;
- downloads: active jobs plus aggregate/revision, not only two unstructured song-ID sets.

Avoid boolean combinations that permit impossible states such as `connected && manual_offline`, `is_playing && no current song`, or a sync marked both idle and running.

### Commands, events, and queries

Use a tagged protocol with stable names and a protocol version.

Representative commands:

```rust
pub enum CoreCommand {
    Initialize,
    Connect(ConnectParams),
    Disconnect,
    SetConnectivity(ConnectivitySettings),
    ReportPlatform(PlatformEvent),
    StartSync(SyncKind),
    CancelOperation(OperationId),
    PlaySelection { song_id: String, song_ids: Vec<String> },
    TogglePlayback,
    Pause,
    Resume,
    Stop,
    SeekTo { seconds: f64 },
    SeekBy { seconds: f64 },
    NavigateQueue(QueueNavigation),
    MutateQueue(QueueMutation),
    SetAudioProcessing(AudioProcessingSettings),
    SetPlaylistOffline { playlist_id: String, enabled: bool },
    RunBackgroundTick,
    Shutdown,
}
```

Queries remain explicit but should be much fewer than the current mutation surface:

- `GetSnapshot` for complete operational reconciliation;
- paged/read-only library/search/playlist queries;
- diagnostics and job history where needed.

Every command result should include:

```text
protocol_version, command_id, accepted_revision, status, value/error
```

Every event should include:

```text
protocol_version, stream_id, event_id, revision, cause_command_id, kind, payload
```

Use one event channel. Domain snapshots (`PlaybackProjection`, `SyncSnapshot`, `DownloadDelta`) can vary in payload size but share one ordering/revision contract. `GetSnapshot` repairs missed events after foregrounding or adapter recreation.

### Transition invariants

The implementation is acceptable only if these invariants are enforced in code and tests:

1. Exactly one `StereodromeRuntime` owns a data directory in a process.
2. Only the runtime actor mutates operational state.
3. Every mutation has a total order and advances `revision` when externally visible state changes.
4. Queue and playback cannot identify different active songs after a completed transition.
5. An async completion can commit only if its operation ID/generation is still current.
6. A command emits its resulting state before reporting success to an adapter, or the result contains the same revision for deterministic reconciliation.
7. Durable state is persisted before a transition is announced as durable.
8. Cancellation is explicit state and every long-running effect has a bounded shutdown path.
9. Platform adapters cannot directly mutate queue, playback, sync, or connection state.
10. React suspension cannot prevent playback, queue, persistence, media-session, or background-job transitions.

### Transaction semantics for playback

External audio effects cannot be part of a SQLite transaction, so use a state-machine reservation/saga rather than pretending the operation is atomic.

Example `Next`:

1. Actor validates the command and records `Playback::Preparing { operation_id, from, target }`.
2. Effect resolves/downloads/decodes the target and asks the audio engine to start it.
3. Completion returns with the same `operation_id` and audio playback identity.
4. Actor verifies the operation is still current.
5. Actor commits queue index + persisted playback state, transitions to `Playing`, and emits one revision.
6. On failure/cancellation, actor restores the previous stable state or transitions to an explicit recoverable failure. It never leaves queue and audio silently split.

The same pattern applies to connection attempts, settings reapplication, sync, backup import, and saved-playlist reconciliation.

## Lean boundary designs

### `stereodrome-ffi`

Target responsibility: memory-safe C ABI adaptation only.

Keep approximately this surface:

```c
void *stereodrome_runtime_new(const char *data_dir);
void stereodrome_runtime_destroy(void *runtime);
char *stereodrome_runtime_dispatch(void *runtime, const char *command_json);
char *stereodrome_runtime_snapshot(void *runtime);
void stereodrome_runtime_set_event_callback(void *runtime, callback, void *context);
void stereodrome_string_free(char *value);
```

Prefer instance-bound callbacks with a context pointer over process-global callback slots. The current stream-ID filtering can remain as defense in depth, but it should not be the primary lifetime mechanism.

Move out of FFI:

- audio monitor policy;
- sync/download/prefetch state;
- background worker creation;
- command-specific cancellation/emission lists;
- playback snapshot construction;
- business-level payload structs and dispatch logic.

An initial target of under roughly 800 lines is realistic; line count is a guardrail, not the architectural goal.

### Swift/Kotlin

Native adapters retain only:

- runtime lifetime and callback registration;
- serial/nonblocking handoff required by platform APIs;
- audio session/focus acquisition and event reporting;
- iOS now-playing / Android Media3 projection;
- Android foreground service lifecycle;
- forwarding events to Expo when JS is alive;
- platform logging and permission/capability reporting.

Delete native snapshot queries used to decide toggle/play policy. Remote controls send typed intents such as `TogglePlayback` or `ReportPlatform(AudioFocusLost)` and apply the projection emitted by Rust.

Generate native protocol models if practical. If JSON remains, parse one generated/stable projection shape instead of independently coercing every field in Swift and Kotlin.

### React Native

Replace the two operational providers with one small external store backed by runtime snapshots/events:

```text
mobile/src/core/
  protocol.generated.ts
  client.ts             # initialize/dispatch/snapshot/subscribe
  store.ts              # monotonic revision application
  selectors.ts          # usePlayback, useConnectivity, useDownloads
```

Use `useSyncExternalStore` (or an equivalent single subscription) for operational state. Keep TanStack Query for paged library/read models, but invalidate by `library_revision`/domain events rather than maintaining a hard-coded list of query keys inside lifecycle orchestration.

React actions become one call each:

```ts
core.dispatch({ type: "play-selection", song_id, song_ids });
core.dispatch({ type: "seek-by", seconds: 10 });
core.dispatch({ type: "set-manual-offline", enabled: true });
```

Remove from React:

- playback restoration and prepare-next sequencing;
- transport/queue action locks;
- seek-drain policy;
- status-generation and pending-foreground flags;
- online/offline operational decisions;
- sync completion bookkeeping;
- duplicate playback/file-state refs and local interpolation policy beyond display-only position interpolation;
- most hand-written response validators and one-wrapper-per-method boilerplate.

Keep in React:

- navigation and selected UI item;
- query pagination/search debounce;
- display-only local position interpolation between authoritative snapshots;
- mobile-only visual preferences such as click-wheel handedness;
- user-facing error presentation.

## Migration plan

### Phase 0 — Characterize and establish budgets

Implementation artifacts:

- deterministic boundary doubles: `crates/stereodrome-core/src/test_support.rs`;
- Rust persistence/navigation/job-exclusion characterization tests in `stereodrome-core` and `stereodrome-ffi`;
- legacy command fixtures: `crates/stereodrome-ffi/tests/fixtures/legacy-command-contract.json`;
- cross-layer playback fixture: `mobile/modules/stereodrome-core/fixtures/playback-snapshot.json`;
- TypeScript and Android consumers of the shared playback fixture;
- code-observed platform baseline and device worksheet: `docs/MOBILE_PHASE0_BASELINE.md`.

Before moving ownership:

1. Add end-to-end Rust tests for current command sequences: cold restore, play selection, rapid next/previous, clear during prepare, seek burst, settings reapply, offline transition, sync collision, saved-playlist reconcile, backup while jobs run, shutdown during prefetch.
2. Add protocol fixture tests that deserialize/serialize commands, results, events, and projections across Rust/TypeScript/native expectations.
3. Add deterministic test doubles for audio, clock, network/server, repository, and event sink.
4. Record baseline native/mobile behavior on both platforms.

Exit criteria:

- failures in later phases can be classified as intended protocol changes or regressions;
- no architecture work relies only on manual playback testing.

### Phase 1 — Introduce the runtime shell without changing behavior

1. Split `stereodrome-core/src/lib.rs` into domain modules while preserving its public API.
2. Add `CoreCommand`, `CoreEvent`, `CoreSnapshot`, structured errors, command IDs, operation IDs, and protocol version.
3. Add `StereodromeRuntimeHandle` and one mailbox.
4. Initially route existing core methods through effects/adapters; do not rewrite algorithms.
5. Make `stereodrome-ffi` dispatch typed commands, while retaining compatibility aliases for existing method strings.

Exit criteria:

- one runtime instance serves all new commands;
- compatibility tests prove old and new paths produce equivalent results;
- no new operation is added to legacy string dispatch.

### Phase 2 — Move jobs and connectivity into the runtime

Move first because these domains do not require the highest-risk audio transition work:

1. Connection/session/offline state.
2. Sync job ownership and due-job scheduling.
3. Download, offline-playlist, and prefetch job registry/cancellation.
4. Unified event stream and operational snapshot.
5. Remove secondary `StereodromeCore` instances and per-job Tokio runtimes.

React/Expo should report lifecycle/network/background ticks, not implement policy.

Exit criteria:

- `LibrarySyncStatus.active_job` is authoritative in core without an FFI overlay;
- one runtime owns all tasks and shuts them down with bounded cancellation;
- sync/offline/download state survives missed JS events via `GetSnapshot`;
- backup exclusion is a runtime invariant, not a cross-mutex inspection in FFI.

### Phase 3 — Move complete playback orchestration into the runtime

1. Introduce an `AudioPort` around `stereodrome-audio::AudioPlayer` for testability.
2. Move snapshot construction, playback monitor inputs, prepare/commit navigation, restore, scrobble progress, gapless/crossfade, and prefetch triggers behind playback state transitions.
3. Replace separate queue/audio commands with intent-level commands.
4. Make platform interruption/focus/route events explicit runtime inputs.
5. Preserve direct native media-session projection from Rust events.

Exit criteria:

- one `PlaySelection` command replaces the current JS call chain;
- one `ClearPlayback` transition clears queue/audio/persistence consistently;
- native, JS, monitor, and background inputs are serialized by the same actor;
- stale async preparation cannot change the active song;
- playback tests run with a fake audio engine and fake clock.

### Phase 4 — Replace mobile orchestration with a thin store

1. Add generated protocol types and the small `client/store/selectors` layer.
2. Migrate screens to selectors/actions.
3. Remove `PlaybackContext` orchestration.
4. Remove operational parts of `StereodromeContext`; retain or rename only UI-facing composition if necessary.
5. Replace hard-coded query-key invalidation with domain revision events.
6. Collapse duplicate app-state listeners to one lifecycle reporter plus TanStack's UI focus integration.

Exit criteria:

- UI actions issue one intent each;
- no React effect is required for playback continuation, queue progression, connection restore, sync completion, or offline reconciliation;
- foregrounding performs one snapshot reconciliation rather than several independent reads.

### Phase 5 — Thin native adapters

1. Use instance-bound callback context/lifetime.
2. Collapse to one event stream.
3. Replace native policy queries with typed intents.
4. Generate/share `PlatformProjection` parsing.
5. Keep platform-only focus/session/service behavior and report outcomes as events.

Exit criteria:

- Swift/Kotlin contain no queue, restore, sync, download, or playback transition policy;
- OS controls remain correct while JS is suspended;
- runtime recreation cannot receive stale callbacks from the previous instance.

### Phase 6 — Adopt the runtime on desktop

1. Wrap the runtime in Tauri state.
2. Convert Tauri commands into adapter calls and events into Tauri emissions.
3. Retire duplicate desktop queue/playback/client/job ownership incrementally.
4. Preserve desktop-only tray, windowing, media keys, notifications, and spectrum UI adapters.

Exit criteria:

- desktop and mobile use the same transition policy and persistence paths;
- queue/playback fixes are written once;
- `src-tauri::AppState` no longer duplicates core operational state.

### Phase 7 — Remove compatibility paths

After both platforms are stable:

- delete legacy method-string dispatch and wrapper methods;
- delete separate playback callback/event channel;
- remove FFI state/job/monitor implementations now owned by runtime;
- delete manual TS field validators superseded by generated protocol contracts;
- update or retire historical sections in `docs/MOBILE.md` and `docs/MOBILE_PLAYBACK_SYNC.md`.

## Validation strategy

### State-machine tests

Use table-driven and model/property tests for:

- allowed/forbidden transitions from every playback/connectivity/job state;
- invariant preservation after every command and effect completion;
- stale completion rejection by operation ID;
- cancellation and shutdown at each await point;
- event/revision monotonicity;
- idempotence of duplicate platform/background events;
- no impossible queue/playback combinations.

### Fault injection

Inject failures at boundaries, not only happy-path service calls:

- database busy/transaction failure;
- server timeout/auth loss/network transition;
- download interruption and corrupt cached audio;
- audio prepare/start/seek/pause/output-rebuild failure;
- callback consumer temporarily absent;
- app foreground/background during every long operation;
- runtime destroy during sync, prefetch, or playback preparation;
- duplicate and reordered completion events in tests.

### Protocol tests

- Rust golden fixtures consumed by TypeScript and native tests.
- Protocol version mismatch fails once at initialization with a structured error.
- Every command/event variant has a fixture and round-trip test.
- CI checks generated files are current.

### Required repository checks per phase

Run the checks for every affected boundary as specified by `AGENTS.md`:

- `cargo fmt --check`
- `cargo clippy -p stereodrome-core -p stereodrome-ffi -- -D warnings`
- `cargo test -p stereodrome-core -p stereodrome-ffi`
- from `mobile`: `vp check`, `vp run typecheck`, and `vp run rust:check` for native/FFI changes
- for shared audio/desktop adoption: `cargo clippy -p stereodrome -- -D warnings` plus focused desktop playback smoke tests

### Manual mobile matrix

At minimum on iOS and Android:

- cold restore, explicit resume, force-kill/relaunch;
- background auto-advance and end of queue;
- phone/Siri interruption, route loss, Bluetooth/headphone changes;
- rapid remote-control and in-app commands concurrently;
- offline toggle during sync/download/playback;
- background sync tick with app/JS absent;
- clear cache/backup attempt while jobs run;
- corrupt next item during gapless/crossfade preparation;
- low storage and network loss during download;
- Android service recreation and iOS media-services reset.

## Success metrics

Architectural success should be measurable:

- one runtime, one mutation mailbox, one event stream;
- zero business/job state fields in `stereodrome-ffi`;
- zero multi-call React action workflows for core operations;
- zero method-name side-effect allowlists;
- zero secondary core instances for background work;
- one foreground operational reconciliation call;
- all long jobs expose operation ID, state, cancellation, and structured failure;
- mobile works correctly with JS suspended;
- desktop/mobile transition behavior shares the same tests;
- substantial reductions from the current 1,100-line TS bridge, 788-line playback provider, 567-line core provider, and 3,551-line FFI implementation.

Do not optimize only for line count. A small bridge over an implicit, untestable runtime is not lean. The desired reduction must come from eliminating ownership and coordination paths.

## Risks and mitigations

| Risk                                             | Mitigation                                                                                                                |
| ------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------- |
| Actor becomes a new monolith                     | Separate reducer/state/protocol from domain services and effects; keep library data as read models.                       |
| Long work blocks command processing              | Spawn cancellable effects and return completion through operation IDs.                                                    |
| Full snapshots become too large                  | Snapshot operational aggregates/revisions; query library data separately; use domain deltas for large download sets.      |
| Runtime migration breaks working playback        | Characterization tests, compatibility commands, and domain-by-domain cutover; playback moves after jobs/connectivity.     |
| Rust cannot own platform audio-session decisions | Native owns the API call; Rust owns desired behavior and consumes typed success/failure/interruption facts.               |
| Generated protocol adds tooling                  | Start with Rust golden fixtures and generated TypeScript; evaluate UniFFI only after the protocol stabilizes.             |
| Desktop migration expands scope                  | Design platform-neutral seams now, but ship mobile runtime first; migrate desktop as a separate gated phase.              |
| Single actor becomes throughput bottleneck       | Serialize mutations only; execute I/O concurrently; serve safe read models through repository read connections/revisions. |

## Non-goals

- Moving visual navigation, selection, click-wheel behavior, or mobile-only appearance settings into Rust.
- Hiding unavoidable iOS/Android media-session and audio-focus code behind unsafe abstractions.
- Replacing SQLite or rewriting tested queue/cache/sync algorithms solely for architectural purity.
- Adopting UniFFI as a prerequisite. A small, versioned JSON protocol is acceptable during migration.
- Keeping React alive in the background.
- A big-bang mobile and desktop rewrite.

## Recommended first implementation slice

The safest useful starting slice is Phase 0 plus the runtime shell for connectivity/sync:

1. Add protocol/error/operation ID types and golden fixtures.
2. Add a single runtime actor around the existing `StereodromeCore`.
3. Move `MobileSyncState` and due/full/incremental job ownership from FFI into that actor.
4. Emit sync and connectivity through the unified event envelope.
5. Keep current playback APIs untouched for this slice.
6. Convert `librarySyncScheduler.ts` to send only `RunBackgroundTick`.

This slice proves the mailbox, effects, cancellation, snapshots, and event protocol without destabilizing audio. Once it is validated, playback can migrate onto a runtime architecture that has already survived real background work.
