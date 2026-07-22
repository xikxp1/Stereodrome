# Mobile runtime protocol

Phase 1 introduces a versioned Rust command boundary without changing mobile
ownership or playback behavior. The protocol version is currently `1`.

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
compatibility aliases generated inside the Rust handle. Repeating a recently
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

`get-snapshot` returns the complete Phase 1 operational projection: lifecycle,
connectivity, persisted playback, queue, sync, download IDs, domain revisions,
and the last mutating-operation failure.

All runtime events carry:

- `protocol_version`
- `stream_id`
- `event_id`
- `revision`
- `cause_command_id`
- `operation_id`
- a tagged event payload

The in-process Rust handle exposes the ordered event stream through
`StereodromeRuntimeHandle::subscribe`. Bridging this stream to one instance-bound
native callback is intentionally deferred until native adapters are thinned.

## C ABI

The new boundary is:

```c
void *stereodrome_runtime_new(const char *data_dir);
void stereodrome_runtime_destroy(void *runtime);
char *stereodrome_runtime_dispatch(void *runtime, const char *command_json);
char *stereodrome_runtime_snapshot(void *runtime);
void stereodrome_runtime_string_free(char *value);
```

The existing `stereodrome_core_*` ABI remains available. Existing method names
that map directly to `StereodromeCore` operations are compatibility aliases into
the typed mailbox. Audio orchestration and FFI-owned background job methods stay
on the legacy path until their ownership moves in Phases 2 and 3. New operations
must be added only to `CoreCommand`, not to the legacy method-string dispatch.
