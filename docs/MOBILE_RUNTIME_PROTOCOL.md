# Mobile runtime protocol

Phases 1–3 introduce a versioned Rust command boundary and move connectivity,
jobs, and complete playback orchestration into one runtime mailbox. The
protocol version is currently `1`.

## Request

```json
{
  "protocol_version": 1,
  "command_id": 42,
  "command": {
    "type": "set-connectivity",
    "settings": { "manual_offline_enabled": true }
  }
}
```

Caller command IDs must be in `1..2^63`; the high range is reserved for
commands generated inside the Rust handle. Repeating a recently
used ID with the same command returns the result from the bounded replay window
without applying the mutation again. Reusing a retained ID for a different
command returns a structured `conflict` error.

## Result

```json
{
  "protocol_version": 1,
  "command_id": 42,
  "accepted_revision": 7,
  "operation_id": 7,
  "status": "succeeded",
  "value": { "manual_offline_enabled": true }
}
```

Failures use `status: "failed"` and an `error` object with `code`, `message`,
and `retryable`. A successful mutation is persisted, assigned a revision, and
published to the runtime event stream before the result is returned.

## Snapshot and events

`get-snapshot` returns the complete operational projection: lifecycle,
connectivity, live playback and queue capabilities, sync, download IDs, domain
revisions, active operations, and the last mutating-operation failure.

Phase 2 adds authoritative `operations`, `saved_playlist_offline`,
`platform_lifecycle`, and `network_available` fields. Sync `active_job` and
running flags are derived from the runtime operation registry rather than an
adapter overlay. Download, saved-playlist, sync, background-tick, and queue
prefetch effects all receive operation IDs and remain visible until completion
or cancellation.

Platform adapters can now send `report-network`, `report-lifecycle`, and
`run-background-tick`. Background ticks own session restoration, offline policy,
due-job selection, and execution; Expo only registers the OS task and reports
the tick.

Phase 3 adds playback intents including `play-selection`, `clear-playback`,
`navigate-playback`, `toggle-playback`, `seek-to`, `seek-by`, and
`report-platform-playback`. Playback preparation is an operation with an ID;
only the currently reserved operation may commit audio and queue state. The
runtime owns restore, progress/scrobble reporting, gapless/crossfade preparation,
prefetch triggers, and audio notifications. Platform focus, interruption,
route-loss, and media-services-reset facts enter the same mailbox.

All runtime events carry:

- `protocol_version`
- `stream_id`
- `event_id`
- `revision`
- `cause_command_id`
- `operation_id`
- a tagged event payload

The in-process Rust handle exposes the ordered event stream through
`StereodromeRuntimeHandle::subscribe`. The C ABI forwards that same `CoreEvent`
JSON without an adapter envelope. Swift, Kotlin, and React Native consume the
`snapshot-changed` event; native projects `snapshot.playback` directly to the OS
media session.

## C ABI

The new boundary is:

```c
void *stereodrome_runtime_new(const char *data_dir);
void stereodrome_runtime_destroy(void *runtime);
char *stereodrome_runtime_dispatch(void *runtime, const char *command_json);
char *stereodrome_runtime_snapshot(void *runtime);
void stereodrome_runtime_set_event_callback(
    void *runtime,
    void (*callback)(const char *event, void *context),
    void *context);
void stereodrome_string_free(char *value);
```

This is the only mobile command ABI. Operations are added to `CoreCommand` and
sent through `stereodrome_runtime_dispatch`; there is no method-string dispatch
or command-specific policy in `stereodrome-ffi`.
