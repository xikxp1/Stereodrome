# Mobile Phase 0 Behavior Baseline

Status: historical baseline captured at commit `12436c9`. The runtime cutover
is complete; Phase 7 removed the legacy fixtures and adapters named by the
original baseline.

This document records the behavior that the Rust runtime refactor must preserve or intentionally change. It is a verification artifact, not a claim that every platform scenario has already passed on hardware.

## Automated baseline

Phase 0 adds the following executable contracts:

| Contract                                                  | Location                                                               | Evidence                                                                                                                                     |
| --------------------------------------------------------- | ---------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Queue and playback persistence across a cold core restart | `crates/stereodrome-core/src/lib.rs` tests                             | Queue items/current index and persisted playback position/intent restore together.                                                           |
| Repeated next/previous ordering and durability            | `crates/stereodrome-core/src/lib.rs` tests                             | Repeated navigation returns to the expected item and the final index survives restart.                                                       |
| Queue clear and audio-setting clamping/persistence        | `crates/stereodrome-core/src/lib.rs` tests                             | Clear resets queue policy; clamped settings survive restart.                                                                                 |
| Typed runtime command/result envelopes                    | `crates/stereodrome-ffi/src/lib.rs` tests                              | Versioned typed dispatch and structured protocol mismatch errors cross the C ABI.                                                            |
| Concurrent job rejection and backup exclusion             | `crates/stereodrome-core/src/runtime` tests                            | A second sync is rejected and backup remains unavailable while runtime-owned work is active.                                                 |
| Runtime playback projection shape                         | `mobile/modules/stereodrome-core/fixtures/runtime-snapshot-event.json` | Android consumes the same `snapshot-changed` event shape emitted by the runtime.                                                             |
| Deterministic future-runtime boundaries                   | `crates/stereodrome-core/src/test_support.rs`                          | Manual clock, fake audio/server/repository, and recording event sink support ordered calls and injected failures without external resources. |

Existing tests continue to characterize queue semantics, cache events, prefetch cancellation/generations, failed navigation preparation, event sequencing, backup transactions, offline behavior, sync due-time calculation, and interrupted download finalization.

## Code-observed platform behavior

### Shared behavior

- Rust owns decoded audio output, live queue mutation, playback monitoring, cache-backed preparation, DSP settings, scrobble progress, gapless/crossfade transitions, and revisioned runtime snapshots.
- Native applies playback from `snapshot-changed` events to the OS media session before forwarding the same event to React Native.
- React Native accepts only monotonically increasing runtime revisions.
- React Native reconciles operational state with one runtime snapshot.
- Persisted playback restores paused; explicit user action is required to resume after process recreation.

### iOS

Observed in `mobile/modules/stereodrome-core/ios/StereodromeCoreModule.swift`:

- Playback-capable commands acquire an `AVAudioSession` before entering Rust.
- Failed playback commands roll back a newly acquired session.
- Pause retains session ownership; stop and stopped/unavailable projections release it.
- interruption begin pauses Rust when it was playing;
- interruption end resumes only when iOS allows it and playback was active before interruption;
- old audio route loss pauses playback;
- media-services reset reconfigures the session, restores projection, and requests Rust output rebuild;
- Control Center commands are acknowledged immediately and dispatched on a serial background queue;
- now-playing metadata/capabilities are projected directly from Rust snapshots.

### Android

Observed in `mobile/modules/stereodrome-core/android/src/main/java/expo/modules/stereodromecore`:

- Playback-capable commands request audio focus before entering Rust.
- Failed commands roll back a newly acquired focus lease.
- Playback snapshots synchronously update the Media3 projection before returning through JNI.
- Active projections start/retain the media-session service; stopped projections clear it.
- Media3 commands are serialized through `StereodromeCoreCommandQueue`.
- Focus loss and platform media controls call Rust directly.
- Media3 exposes play/pause, seek, next, previous, and stop according to snapshot capabilities.

## Device baseline worksheet

Run this matrix on at least one supported physical iOS device and one supported physical Android device before Phase 3 changes playback ownership. Store logs/screenshots with the test build identifier.

Result values: `pass`, `fail`, `blocked`, `not-run`.

| Scenario                                                                | iOS     | Android | Required evidence                                       |
| ----------------------------------------------------------------------- | ------- | ------- | ------------------------------------------------------- |
| Cold start with persisted queue restores paused song/position           | not-run | not-run | App and OS projection screenshot; Rust snapshot log.    |
| Explicit resume after cold restore starts the correct item              | not-run | not-run | Audible item matches queue/current metadata.            |
| Background auto-advance updates lock-screen metadata                    | not-run | not-run | Before/after lock-screen capture and snapshot sequence. |
| End of queue clears or pauses OS projection correctly                   | not-run | not-run | Final Rust snapshot and OS state.                       |
| Rapid in-app next/previous remains ordered                              | not-run | not-run | Command/snapshot log with final current item.           |
| Concurrent OS and in-app transport commands converge                    | not-run | not-run | Command order and final snapshot.                       |
| Burst seeks converge on expected position                               | not-run | not-run | Requested deltas, final Rust position, OS position.     |
| Pause/resume through phone or assistant interruption                    | not-run | not-run | Interruption log and final snapshot.                    |
| Headphone/Bluetooth route loss pauses consistently                      | not-run | not-run | Native event, Rust state, OS projection.                |
| Output/media-service recreation recovers playback controls              | not-run | not-run | Rebuild/service lifecycle log.                          |
| Manual offline toggle during playback preserves cached playback         | not-run | not-run | Connectivity and playback snapshots.                    |
| Manual offline toggle during sync/download reaches a stable job state   | not-run | not-run | Sync/download status events and final snapshot.         |
| Background sync tick with React suspended completes or defers cleanly   | not-run | not-run | OS task result plus Rust sync status.                   |
| Backup is rejected while sync/download/prefetch work is active          | not-run | not-run | Structured/current error string and active job status.  |
| Corrupt next item during gapless/crossfade does not advance incorrectly | not-run | not-run | Queue and playback snapshots around failure.            |
| Force-kill/relaunch does not leave stale OS controls                    | not-run | not-run | OS state before kill and after relaunch.                |

## Capture format

For each manual run record:

```text
build/commit:
platform/device/OS:
scenario:
preconditions:
steps:
result:
first unexpected revision/sequence:
Rust logs:
native logs:
screenshots/video:
notes:
```

Any failure discovered here becomes a named characterization test or an explicitly accepted behavior change before the relevant refactor phase proceeds.
