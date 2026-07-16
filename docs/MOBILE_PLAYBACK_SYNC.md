# Mobile Playback State Sync: Problems and Refactor Plan

Status: historical proposal, investigated against branch `feat/cef` on 2026-07-06. The Rust snapshot announcer and direct native OS projection are now implemented. As of 2026-07-16, snapshot callbacks also apply OS state synchronously and native teardown clears it deterministically; line numbers below remain point-in-time anchors, not contracts.

This document explains why mobile playback state diverges between the Rust audio core, the OS media session (lock screen / notification / Control Center), and the React Native UI — "music is playing but the app or OS says it is not, and vice versa" — and proposes a refactor to eliminate the divergence class rather than continuing to patch instances of it.

Predecessor: `docs/MOBILE.md` (feature parity plan). That plan is largely executed — playback is now fully Rust-owned; `react-native-track-player` is gone. This document covers the architecture problem that emerged from that migration.

## TL;DR

There are five copies of playback state and no push channel between them. The Rust core is the declared source of truth, but:

1. **Rust cannot notify anyone.** The only callback across the FFI boundary is logging (`crates/stereodrome-ffi/src/lib.rs:95`). Everything above Rust is polling or push-on-explicit-command.
2. **Rust's own truth is partly fictional.** Position is wall-clock time, `is_playing` is a flag nothing ties to device health, and the cpal output stream is opened once and never health-checked. Rust can believe it is playing while the device renders nothing, and its position advances regardless.
3. **The most state-changing component notifies no one.** The 100 ms monitor thread in `stereodrome-ffi` auto-advances tracks, triggers crossfade, and stops at end of queue — silently.
4. **The OS media session is updated through the least reliable path.** Rust → JS poll → React state → JS push → native. JS is suspended by iOS exactly when this path matters most (backgrounded playback, end of queue).

The fix is structural: make Rust the single **announcer** as well as the single source of truth (versioned snapshot events over an FFI callback), let the native layer drive the OS media session directly from those events, ground Rust's `is_playing`/position in actual sink consumption instead of the wall clock, and make queue transitions transactional with audio.

---

## 1. Current architecture

### 1.1 Layers and state holders

There is no JS audio library (`expo-av`, `expo-audio`, `react-native-track-player` — none present in `mobile/package.json`). All audio is Rust (rodio/cpal); all OS integration is the custom Expo module `mobile/modules/stereodrome-core`.

| #   | Layer                                                                                                                                                                    | State held                                                                                                                                                                           | Updated by                                                                                  | Staleness                                                             |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| 1   | Audio engine — `crates/stereodrome-audio/src/player.rs`                                                                                                                  | `is_playing` atomic (`:156`), `current_song`, `playback_start: Instant`, `paused_position`, `duration`, gapless segments (`PlaybackInner`, `:130`)                                   | Transport commands; end-of-track sink-empty detection (`:1160`)                             | Source of truth, but see §2.2 — can diverge from the actual device    |
| 2   | Core queue + persistence — `crates/stereodrome-core`                                                                                                                     | `PlayQueue.current_index`, repeat/shuffle, SQLite `playback_state` row                                                                                                               | JS-initiated mutations; the FFI monitor thread                                              | Can point at a different song than layer 1 during transitions         |
| 3   | FFI monitor thread — `crates/stereodrome-ffi/src/lib.rs:930-1073`                                                                                                        | `last_segment_idx`, `last_report`, throttle timestamps                                                                                                                               | Its own 100 ms tick                                                                         | Freezes entirely under iOS process suspension                         |
| 4   | JS — `mobile/src/context/PlaybackContext.tsx`                                                                                                                            | React state + parallel refs for isPlaying/song/position/duration/index/queue (`:115-141`), plus `expectedAudioSongIdRef`, `restoredStartPositionRef`, `playbackActivatedThisProcess` | Adaptive poll (1 s / 5 s / 15 s, `:459-466`); command responses; foreground refresh         | Poll interval at best; unbounded while JS is suspended                |
| 5   | OS media session — iOS `MPNowPlayingInfoCenter` (`StereodromeCoreModule.swift`), Android Media3 session (`StereodromeMediaPlayer.kt`, `StereodromeMediaSessionState.kt`) | Now-playing metadata, playback state, command availability; Android also caches `nowPlayingInfo` + `serviceStarted` statics                                                          | JS pushes via `nativeMediaControls.ts` (deduped); native-side refresh after transport calls | Stale whenever the JS path is asleep and no native-side refresh fired |

The actual audio device output is a sixth, **unobserved** truth: nothing reads back what the device rendered.

### 1.2 How state propagates today

```
                 ┌────────────────────────────────────────────────────────┐
                 │ Rust audio engine (is_playing, wall-clock position)    │
                 │        ▲ commands (ack ≤1s: pause/resume/stop/seek)    │
                 └──┬─────┴───────────────────────────────────────────────┘
      no events!    │ audioGetStatus (pull)
                    │                          ┌──────────────────────────┐
   ┌────────────────┴───┐  100ms tick, mutates │ FFI monitor thread       │
   │                    │◄─────────────────────┤ advance/crossfade/stop   │
   │                    │      notifies no one └──────────────────────────┘
   │  JS PlaybackContext│
   │  poll 1s/5s/15s    │  ◄── suspended by iOS when backgrounded/idle
   └────────┬───────────┘
            │ nativeMediaControls.sync() (deduped by infoKey/progressKey)
            ▼
   ┌────────────────────┐   native-playback-invalidated (ping, no payload)
   │ Native module      │ ──────────────────────────────────────────► JS
   │ MPNowPlaying/Media3│   + native-side refresh after transport calls
   └────────────────────┘
```

Sync paths, exhaustively:

1. **JS adaptive poll** (`PlaybackContext.tsx:426-478`): `audioGetStatus` + `getQueue` (two separate FFI calls), 1 s foreground-playing / 5 s background-playing or paused / 15 s idle. The primary — and often only — Rust→JS reconciliation.
2. **JS→native push** (`PlaybackContext.tsx:672-694` → `nativeMediaControls.ts:27-107`): fires on React state change, deduped by `infoKey`/`progressKey`. Note `infoKey` excludes `is_playing`.
3. **JS-local "core events"** (`stereodromeCore.ts`): `queue-changed` is emitted _by JS, after JS-initiated mutations only_ (`emitCoreEvent` at `:507/:513/:519`). Rust's autonomous queue advances emit nothing. The `playback-state` event name (`:39`) is declared but never emitted nor subscribed — a vestige of the push channel that was never built.
4. **`native-playback-invalidated`** (iOS `sendEvent` `StereodromeCoreModule.swift:395`; Android `StereodromeCoreModule.kt:13-15`): a payload-less "go refetch" ping from native to JS after native-initiated changes. Dropped/deferred if JS is suspended.
5. **AppState foreground refresh** (`PlaybackContext.tsx:402-424`): the main catch-up after suspension.
6. **Native-side self-refresh after transport calls** (iOS `call` wrapper `StereodromeCoreModule.swift:78-97`, Android `StereodromeCoreModule.kt:27-39`): after pause/resume/seek/play/stop, native re-reads Rust status and updates the OS session directly — a patch acknowledging that the JS path can't be trusted. It only covers transitions that go _through_ `call`; the monitor thread's transitions bypass it entirely.
7. **Rust monitor → persistence/Last.fm only** (`report_mobile_progress`, `lib.rs:1082-1114`): throttled progress reports feed scrobbling and the persisted snapshot. No UI-facing output.

### 1.3 The patch trail

Recent history is a sequence of point fixes to this design: "authorative native module controls", "synchronous calls from Rust audio process to native media core", "native session interruption handling", "don't release audio session on pause", "keep media session service handle on pause on Android", "restore previous media state paused", ack-synchronized transport commands, the `expectedAudioSongIdRef` guard, native-side post-transport refresh. Each fixed one race; none removed the race-generating structure.

---

## 2. Structural problems

**P1 — No push channel from Rust.** The FFI exposes exactly one callback: logging (`stereodrome_core_set_log_callback`, `lib.rs:95`). State changes originating in Rust (auto-advance, end-of-queue stop, crossfade, gapless segment change) are invisible until someone polls — and the poller (JS) is suspended precisely when playback runs unattended in the background.

**P2 — Rust ground truth is decoupled from the device.**

- Position is pure wall clock: `playback_start.elapsed() + paused_position`, clamped to duration (`player.rs:188-197`). Rodio's actual consumed-samples position is never consulted.
- `is_playing` is flipped false only by explicit Pause/Stop or by sink-drained end-of-track detection (`player.rs:857/894/1169`). Nothing ties it to whether the device callback is still pulling samples.
- The `OutputStream` is opened once at thread start (`player.rs:722`) and never health-checked or rebuilt. If it dies at startup the thread just returns (`:731`), and all subsequent transport commands are silently acknowledged as no-ops.
- Transport ack timeouts log a warning and return `Ok` (`player.rs:447-449`) — a wedged audio thread is invisible to every caller.

**P3 — The most active mutator notifies no one.** The monitor thread (`lib.rs:930-1073`) advances the core queue on gapless segment change (`:965-988`), triggers crossfade (`:992-1030`), advances on track end (`:1032-1054`), and stops/clears at end of queue (`:1055-1062`). All of these change what the user should see on the lock screen; none of them emit anything toward JS or the native session.

**P4 — Five unversioned copies reconciled ad hoc.** There is no sequence number or generation on playback state. A stale `audioGetStatus` response can overwrite fresher state (see S7); dedup caches in `nativeMediaControls.ts` and Android's `StereodromeMediaSessionState` decide "nothing changed" from incomplete keys (see S5); `expectedAudioSongIdRef` papers over one specific race and creates another (S9).

**P5 — Transitions are not transactional.** Crossfade advances the core queue (`lib.rs:1268`) the moment the command is _enqueued_, before the audio thread has decoded anything. If decode then fails, the audio thread restores the old sink (`player.rs:1104-1109`) but `crossfade_initiated` stays true (reset only in the success branch, `player.rs:1097`), and the FFI-level resets (`lib.rs:1011-1013`) don't cover this path. Result: queue index and audible song disagree, and end-of-track advance is permanently suppressed by the `!crossfade_initiated` guard (`lib.rs:1035`) — playback wedges.

**P6 — The monitor is suspension-blind.** It is a `thread::sleep(100ms)` loop keyed to `Instant`s. Across an iOS suspend/resume, wall-clock elapsed and device-rendered position diverge arbitrarily; on resume the monitor may fire spurious advances, scrobbles, or crossfades from a position jump it cannot distinguish from real playback.

---

## 3. Concrete divergence scenarios

Grouped by symptom. All are constructible from current code.

### A. "Shows playing, but nothing is audible"

**S1 — End of queue while backgrounded (iOS).** Last track drains → monitor calls `clear_finished_state` (`lib.rs:1055-1062`). No notification. iOS releases the background-audio assertion, suspending JS — so the poll that would call `nativeMediaControls.clear()` (`PlaybackContext.tsx:446-448`) never runs. Lock screen shows the last song "playing" indefinitely, until the app is foregrounded. This is the most probable match for the reported symptom.

**S2 — Dead output stream.** If iOS kills the audio unit (media services reset, some route changes), the mixer stops pulling samples, so `sink.empty()` never turns true and end-of-track detection (`player.rs:1160`) never fires. Wall-clock position climbs to `duration` and clamps. `audioGetStatus` reports `is_playing=true, position=duration` forever; the monitor's advance requires `!is_playing` (`lib.rs:1032-1035`) and never fires. Frozen "playing" UI, no sound, no recovery path (the known missing `audioRebuildOutput`).

**S3 — Interruption without pause.** An interruption Rust never hears about (e.g. iOS delivers `.appWasSuspended`, deliberately ignored at `StereodromeCoreModule.swift:316-321`) leaves `is_playing=true` while output is silenced. Wall-clock position keeps advancing → the 50% scrobble can fire for audio never heard, and the monitor can auto-advance at the phantom track boundary. A subsequent lock-screen "play" maps to `audioResume` on an already-"playing" core — a no-op; the user taps play and nothing happens.

### B. "Playing, but the OS/UI shows wrong or no state"

**S4 — Auto-advance shows the previous song.** Monitor advances (gapless `lib.rs:965-988` or track end `:1042-1054`); is*playing stays true. No event fires; the native post-transport refresh doesn't apply (the monitor bypasses `call`). The lock screen shows the \_previous* song's title/artwork/duration for up to 1 s (foreground) / 5 s (background) — or, if JS is suspended and tracks are short, the lock screen can run a full track behind indefinitely.

**S5 — Android: no media notification after service death.** `serviceStarted` is a process-static boolean (`StereodromeMediaSessionState.kt:11`) that survives the OS killing the actual foreground service. On resume of the same song, `nativeMediaControls` skips `setNowPlayingInfo` (its `infoKey` excludes `is_playing`, `nativeMediaControls.ts:48-61`) and `updateProgress` only restarts the service `if (isPlaying && !serviceStarted)` — false because of the stale flag. Audio plays with no notification/controls.

**S6 — Android focus-loss asymmetry.** Becoming-noisy pauses Rust _and_ refreshes the notification (`StereodromeMediaSessionService.kt:20-25`). Plain audio-focus loss pauses Rust and pings JS but never refreshes the notification (`StereodromeAudioFocus.kt:22-28` → `StereodromeCoreBridge.kt:41-47`). If JS is asleep, the notification keeps showing "playing".

### C. Races and wedges

**S7 — Poll races user pause.** The poll doesn't take the `"transport"` action lock. A poll's `audioGetStatus` issued just before `audioPause` can resolve after it and overwrite `isPlaying` back to true, which the sync effect then pushes to the OS. UI and lock screen bounce pause→playing→paused.

**S8 — Crossfade decode failure wedges playback.** P5's scenario: queue advanced, audio still on the old song, `crossfade_initiated` stuck true, auto-advance suppressed. Requires a failed decode of the next track (corrupt cache entry, truncated download) mid-crossfade window.

**S9 — `expectedAudioSongIdRef` wedge.** Set before `audioPlayCurrent` (`PlaybackContext.tsx:243`), cleared only when a matching status arrives (`:180`) or on `clearQueue`. If `audioPlayCurrent` throws before any matching status is observed, every subsequent poll updates only `isPlaying` (`:171-178`) — song/position/duration freeze on the previous track.

**S10 — Resume after natural track end is a silent no-op.** End-of-track sets `is_playing=false` with the sink drained; `Resume` guards on `sink.is_paused()` (`player.rs:864`), which a drained sink is not. The command acks successfully and does nothing; the play button appears broken.

**S11 — Status and queue fetched non-atomically.** The poll and `refreshFromNativePlayback` issue `audioGetStatus` and `getQueue` as two FFI calls; the 100 ms monitor can advance between them, producing a transiently inconsistent song/index pairing.

**S12 — Lock-screen play dead after cold restore.** `playbackActivatedThisProcess` starts false, so JS pushes `can_play:false`, native disables the play command (`StereodromeCoreModule.swift:536-540`). A restored session shows on the lock screen with a play button that does nothing until the user opens the app and plays from there.

### Dead/vestigial code found along the way

- `"playback-state"` event name: declared, never emitted, never subscribed (`stereodromeCore.ts:39`).
- `audioCrossfadeNext` / `reportPlaybackProgress` JS wrappers: unused from JS (monitor-driven).
- `toggle()` in `StereodromeCoreBridge.kt:91-105`: no callers.

---

## 4. Target architecture

### 4.1 Principles

1. **Single source of truth, single announcer.** Rust owns playback state _and_ is the only component that announces changes — including its own autonomous ones. Everything else renders.
2. **The OS media session is driven natively from Rust events.** JS is removed from the OS-session update path entirely. JS being suspended must have zero effect on lock-screen correctness.
3. **Engine state is grounded in the device.** `is_playing` and position derive from what the sink actually consumed, with explicit `stalled` detection and a stream-rebuild path.
4. **Transitions are transactional.** The queue index moves only when audio confirms the new source is live.
5. **State application is monotonic.** Every snapshot carries a sequence number; consumers apply a snapshot only if its seq exceeds the last applied one. Stale responses become harmless.

### 4.2 Target data flow

```
┌───────────────────────────────────────────────────────────────┐
│ Rust core                                                     │
│  engine (device-grounded is_playing/position, stall watchdog) │
│  queue + monitor (transactional transitions)                  │
│        │                                                      │
│        │ on every change: PlaybackSnapshot { seq, ... }       │
│        ▼                                                      │
│  stereodrome_core_set_playback_callback  ── FFI callback ──┐  │
└────────────────────────────────────────────────────────────┼──┘
                                                             ▼
                                      ┌──────────────────────────────┐
                                      │ Native module (Swift/Kotlin) │
                                      │ 1. update MPNowPlaying/Media3│
                                      │    directly from snapshot    │
                                      │ 2. forward snapshot to JS    │
                                      │    via sendEvent (if awake)  │
                                      └──────────────┬───────────────┘
                                                     ▼
                                      ┌──────────────────────────────┐
                                      │ JS PlaybackContext           │
                                      │ single snapshot state,       │
                                      │ seq-monotonic application,   │
                                      │ reconcile on foreground only │
                                      └──────────────────────────────┘
```

### 4.3 Components

**R1 — Playback event channel (the keystone).**

- New FFI registration alongside the log callback: `stereodrome_core_set_playback_callback(cb)`, delivering a JSON `PlaybackSnapshot` (same envelope conventions, `snake_case` fields):
  ```json
  {
    "seq": 1234,
    "is_playing": true,
    "state": "playing | paused | stopped | stalled",
    "song": {
      "id": "...",
      "title": "...",
      "artist": "...",
      "album": "...",
      "duration": 213.4,
      "artwork_uri": "/.../cover_512.jpg"
    },
    "position": 42.1,
    "queue_index": 3,
    "queue_length": 12,
    "can_next": true,
    "can_previous": true,
    "can_play": true,
    "can_seek": true
  }
  ```
- Emitted from one place (a small `PlaybackAnnouncer` in `stereodrome-ffi`) on: transport command applied, monitor transition (advance/crossfade/gapless/end-of-queue), queue mutation affecting the current item, stall/recovery, settings reapply. Deliver the **full snapshot, not an invalidation ping** — a ping requires a read-back that can race the next change; a snapshot with `seq` cannot.
- Rust computes `can_next`/`can_previous`/`can_seek`/`can_play` from queue + restore state, eliminating the JS-side derivation and the `playbackActivatedThisProcess` process flag (fixes S12 by policy: a restored queue item ⇒ `can_play:true`).
- `artwork_uri` points at the local cover cache (Rust already resolves covers); native loads the image itself. Kills the JS artwork cache in `nativeMediaControls.ts`.
- Add one combined `getPlaybackSnapshot` FFI method (same payload) for pull-based reconciliation, replacing the non-atomic `audioGetStatus`+`getQueue` pair (fixes S11).

**R2 — Native-owned media session.**

- iOS: the module holds the last snapshot; on callback, updates `MPNowPlayingInfoCenter` + command availability on its existing serial queue. `notifyNativePlaybackChanged` and the post-transport refresh in the `call` wrapper become redundant and are deleted — the transport command itself triggers a snapshot emission from Rust.
- Android: `StereodromeMediaPlayer`/`StereodromeMediaSessionState` become pure projections of the last snapshot. `serviceStarted` is replaced by asking the actual service state (bind or service-side liveness), and service start/stop decisions key off `state != stopped` in the snapshot (fixes S5). Focus-loss and becoming-noisy handlers just call Rust pause; the notification refresh arrives uniformly via the callback (fixes S6).
- Remote commands keep calling Rust directly (as today), but no longer need bespoke refresh logic afterward.
- JS's `nativeMediaControls.ts` (sync, dedup keys, artwork cache, clear) is deleted; `setNowPlayingInfo`/`updateNowPlayingProgress`/`clearNowPlayingInfo` module methods are removed from the JS API surface.

**R3 — Device-grounded engine state (`crates/stereodrome-audio`; shared with desktop — changes must keep `src-tauri` green, and desktop gets the same robustness for free).**

- Position: derive from the sink's consumed-samples position (rodio exposes the played position on the sink), keeping wall clock only as interpolation between reads. Segment selection for gapless (`get_gapless_state`) keys off consumed samples, not `Instant`.
- Stall watchdog: `is_playing && position not advancing across N ticks` ⇒ `state = stalled`, emit snapshot, stop the wall-clock. This single mechanism catches dead streams, silenced output, and suspension skew (defuses S2/S3 and P6's phantom scrobbles — scrobble/advance logic requires _consumed_ progress, not elapsed time).
- Stream lifecycle: register cpal error handling; on stream death or explicit request, rebuild `OutputStream` and reattach. New FFI method `audioRebuildOutput` so iOS can force a rebuild on `mediaServicesWereReset` (closing the long-known gap).
- Honest errors: transport ack timeout returns `Err`, not `Ok` (`player.rs:447`); `Play`/`CrossfadePlay`/`AppendGapless` gain result reporting (see R4).
- `audioResume` semantics become "ensure playing": if the sink is drained or absent, fall back to playing the current queue item at the persisted position (fixes S10 and makes lock-screen play after cold restore work natively, completing S12).

**R4 — Transactional transitions (`stereodrome-ffi` monitor + engine).**

- Crossfade: the audio thread reports decode success/failure for `CrossfadePlay`; the monitor advances the core queue only on success, and resets `crossfade_initiated` on failure (fixes S8/P5). Same pattern for gapless append.
- Monitor suspension awareness: each tick compares expected vs actual elapsed; a gap beyond a threshold means "we were suspended" → reconcile from consumed-samples state instead of trusting wall-clock deltas for advance/scrobble decisions.
- Every monitor-initiated mutation ends with a snapshot emission (via R1).

**R5 — JS simplification (`PlaybackContext.tsx`).**

- Subscribe to the forwarded snapshot event; apply monotonically by `seq`. This removes: the ref mirror (`isPlayingRef` etc. survive only as an implementation detail if needed), `expectedAudioSongIdRef` (S9 — stale snapshots are rejected by seq, fresh ones are correct by construction), and the poll-vs-action races (S7 — a pre-pause snapshot has a lower seq than the post-pause one).
- Polling shrinks to reconciliation: one `getPlaybackSnapshot` on foreground/app-start, plus a slow safety poll (~30 s) while a song is loaded. The 1 s foreground poll can be kept temporarily as a position ticker or replaced by local interpolation between snapshots (snapshot carries position + is_playing; UI interpolates).
- `queue-changed` stops being a JS-local fiction: queue mutations return state as today, and Rust-initiated queue changes arrive via snapshots.

### 4.4 Explicit non-goals

- No change to desktop behavior (`src-tauri` keeps its own event system; engine changes in `stereodrome-audio` must be additive/compatible).
- No change to DSP, caching, sync, scrobble policy (only _when_ scrobble progress is counted becomes consumption-based).
- No attempt to keep JS runtime alive in background; the design assumes JS is dead whenever inconvenient.

---

## 5. Migration plan

Ordered so every phase ships independently and reduces divergence on its own.

### Phase 0 — Quick wins (independent bug fixes, low risk)

1. Reset `crossfade_initiated` in the decode-failure branch (`player.rs:1104-1109`) and make `crossfade_next_from` tolerate it (S8 wedge).
2. Transport ack timeout returns `Err` (`player.rs:447-449`); callers surface it.
3. Android: refresh the notification on focus-loss pause, same as becoming-noisy (S6); derive service-running from the service itself, not the `serviceStarted` static (S5).
4. JS: clear `expectedAudioSongIdRef` in a `finally` around `audioPlayCurrent` (S9).
5. Interim: include `is_playing` in `nativeMediaControls` `infoKey` (removes a class of skipped pushes until R2 deletes the file).

### Phase 1 — Event channel (R1) + native session ownership (R2)

- Add `PlaybackSnapshot` + `seq` + announcer in `stereodrome-ffi`; new FFI callback registration; `getPlaybackSnapshot` method.
- iOS/Android modules consume snapshots for the OS session; forward to JS via the existing event mechanism.
- JS switches to snapshot events + foreground reconciliation; fast poll demoted to position ticker.
- Follow the Mobile FFI checklist in `AGENTS.md` (ffi → core → native bridges → TS types → `bun run rust:check`).
- **Acceptance:** with the app backgrounded and JS provably suspended: (a) auto-advance updates the lock screen within ~1 s (S4); (b) end of queue clears/pauses the lock-screen state (S1); (c) killing the Android service and resuming shows a correct notification (S5).

### Phase 2 — Device-grounded engine (R3)

- Consumed-samples position + stall watchdog + `stalled` state; stream rebuild + `audioRebuildOutput`; iOS wires `mediaServicesWereReset` to it; "ensure playing" resume semantics.
- **Acceptance:** simulate output death (media services reset / route yank): app shows `stalled` (not perpetual "playing"), recovers on rebuild; no scrobble fires for a track whose audio was silenced (S2, S3); play after natural track end works (S10).

### Phase 3 — Transactional transitions (R4)

- Ack/result reporting for `CrossfadePlay`/`AppendGapless`/`Play`; queue advance moves behind audio confirmation; monitor suspension detection.
- **Acceptance:** corrupt next-track file during crossfade → playback continues on current song, queue index consistent, next advance still fires; suspend/resume across a track boundary neither double-advances nor phantom-scrobbles.

### Phase 4 — Cleanup

- Delete `nativeMediaControls.ts`, now-playing JS module methods, `notifyNativePlaybackChanged`, post-transport native refreshes, `expectedAudioSongIdRef`, `playbackActivatedThisProcess`, the dead `"playback-state"` event name (superseded by the real snapshot event), unused `audioCrossfadeNext`/`reportPlaybackProgress` JS wrappers, Kotlin `toggle()`.
- Update `docs/MOBILE.md` §"Immediate Next Work" pointers or mark superseded sections.

## 6. Validation

Rust (`cargo test -p stereodrome-core -p stereodrome-ffi`, plus new tests):

- Snapshot seq monotonicity across concurrent transport + monitor mutations.
- Monitor state machine with a mocked engine: advance, end-of-queue, crossfade failure, suspension gap.
- Stall watchdog: position frozen while `is_playing` ⇒ `stalled` within N ticks.

Manual QA matrix (per platform):

- Background auto-advance with short tracks: lock screen tracks every change.
- End of queue while locked: controls clear/pause correctly.
- Phone call / Siri mid-track: state pauses or stalls, no scrobble for silent audio, resume works from lock screen.
- Headphone unplug / bluetooth route change: pauses everywhere consistently.
- Cold start with restored session: lock-screen play starts playback without opening the app.
- Android: swipe-kill the task while playing; service/notification behavior; resume path.
- Crossfade with a corrupted cached next track: no wedge.
- Desktop regression pass after Phase 2/3 engine changes (`cargo clippy -p stereodrome -- -D warnings`, desktop playback smoke test) since `stereodrome-audio` is shared.
