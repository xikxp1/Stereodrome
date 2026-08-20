# AGENTS.md

## Repository Layout

- Root frontend: SvelteKit SPA used by the Tauri desktop app.
- Desktop shell/backend: `src-tauri`.
- Shared audio engine: `crates/stereodrome-audio`.
- Shared business logic, persistence, and runtime protocol: `crates/stereodrome-core`.
- Mobile C ABI: `crates/stereodrome-ffi`.
- Vendored Subsonic client fork: `crates/submarine`.
- Mobile app: Expo/React Native app in `mobile`.
- Mobile native module bridge: `mobile/modules/stereodrome-core`, backed by `crates/stereodrome-ffi`.

## Dependency Management

- Rust dependencies: use `cargo add` from the workspace root, or pass `-p <crate>` for a specific crate.
- Root desktop/frontend dependencies: use `vp add` from the repository root.
- Mobile dependencies: use `vp add` from `mobile`.
- Do not use `npm`.
- After root dependency changes, run `vp install` from the repository root to refresh `bun.lock`.
- After mobile dependency changes, run `vp install` from `mobile` to refresh `mobile/bun.lock`.

## Validation

- Run checks after making code changes.
- Root desktop/frontend checks:
  - `vp check`
  - `vp run check:svelte`
- Mobile JS checks (from `mobile`):
  - `vp check`
  - `vp run typecheck`
- Rust workspace checks:
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo test --workspace --all-features`
- Generated protocol check after changing exported Rust types or commands:
  - `vp run protocol:types:check`
- Mobile native bridge checks (from `mobile`) when changing `crates/stereodrome-ffi`, `crates/stereodrome-core`, `mobile/modules/stereodrome-core`, or generated native library artifacts:
  - `vp run rust:check`
- If a change crosses desktop, mobile, and/or shared Rust boundaries, run the checks for every affected area.

## Generated Protocol Types

- `src/lib/types/protocol.generated.ts` and `mobile/src/core/protocol.generated.ts` are generated from the Rust types by `scripts/generate-protocol-types.sh`. Never hand-edit them.
- After changing any exported type or command in `crates/stereodrome-core/src/{models,queue,lastfm,backup,protocol}.rs`, run `vp run protocol:types` and include both generated outputs in the same change. CI fails via `vp run protocol:types:check` when the output is stale.
- To expose a new type, derive `TS` with `#[cfg_attr(feature = "ts", derive(ts_rs::TS))]` and add it to the list in `crates/stereodrome-core/src/bin/export-protocol-types.rs`.
- Command result payloads cannot be derived: the runtime erases them to `serde_json::Value`. The command-to-payload mapping is the hand-maintained `COMMAND_RESULTS` table in `crates/stereodrome-core/src/bin/export-protocol-types.rs`, which generates `CoreCommandValue` for both platforms. Keep it in sync with `runtime/effect.rs` and `runtime/mod.rs`; commands omitted from it resolve to `void` on the client.

## Project Conventions

- Root desktop frontend is SPA-only (`src/routes/+layout.ts` has `ssr = false`): avoid SSR-only patterns.
- Use Svelte 5 runes (`$state`, `$derived`, `$effect`, `$props`) for new root Svelte stateful frontend code.
- Mobile UI is Expo/React Native. Keep mobile UI and platform integration inside `mobile` unless the change is intentionally shared through Rust FFI or shared TypeScript abstractions.
- Keep Rust/TypeScript wire payload fields in `snake_case` unless the shared protocol explicitly defines a rename. Use the generated protocol types instead of duplicating payload shapes by hand.
- Use structured logging (`log` crate / Tauri log plugin / platform logging), not `println!` or `console.log`.
- `crates/submarine` is a vendored fork with local HTTP client, TLS, and streaming changes. Preserve those changes and keep `stereodrome-core` on the local path dependency when updating it.

## Desktop Runtime Commands

New shared-runtime operations should reach the desktop through the generic `core_dispatch` Tauri command, mirroring how mobile dispatches over FFI. Existing dedicated commands may remain as desktop adapters, but prefer the generic path when no desktop-specific work is required:

- Call `dispatch({ type: "..." })` from `src/lib/api/core.ts`. The payload type comes from the generated `CoreCommandValue`, so no hand-written return type is needed.
- Implement a new `CoreCommand` in the shared runtime (`runtime/effect.rs` and/or `runtime/mod.rs`, as appropriate). It needs no command-specific Tauri wrapper or registration. If it returns a value, add it to `COMMAND_RESULTS` and re-run `vp run protocol:types`. The exporter panics if the tag is not a real variant.

Only add a dedicated `#[tauri::command]` when the operation needs desktop-specific work (keyring, windowing, tray, file I/O, desktop settings store, event emission). In that case update all of these:

1. Implement/update the Rust command in `src-tauri/src/commands/*.rs` with `#[tauri::command]`.
2. Re-export it in `src-tauri/src/commands/mod.rs`.
3. Register it in `src-tauri/src/lib.rs` under `tauri::generate_handler!`.
4. Add/update the frontend wrapper in `src/lib/api/commands.ts`.
5. Add/update TypeScript types in `src/lib/types/index.ts` if payload/response shapes changed.

## Mobile Runtime Commands

Mobile runtime operations use one versioned JSON boundary:

- `mobile/src/core/client.ts` sends a `CoreCommandRequest` through the Expo module's `dispatch(commandJson)` method.
- `crates/stereodrome-ffi` forwards that request through `stereodrome_runtime_dispatch`; it does not contain command-specific dispatch policy.
- Adding a normal `CoreCommand` requires shared runtime implementation, generated protocol updates, and mobile callers as needed. It does not require a new C, Swift, Kotlin, or Expo method.

Only change the native boundary for lifecycle, dispatch transport, event callbacks, resource diagnostics, or platform-owned behavior such as media sessions. When that boundary changes, update all affected layers:

1. Update exports and behavior in `crates/stereodrome-ffi`.
2. Update `crates/stereodrome-ffi/include/stereodrome_ffi.h` when C symbols or signatures change.
3. Update both Swift and Kotlin bindings in `mobile/modules/stereodrome-core`.
4. Update the Expo module TypeScript surface and callers in `mobile`.
5. Update `docs/MOBILE_RUNTIME_PROTOCOL.md` when the wire contract changes.
6. Run `vp run rust:check` from `mobile` when native artifacts or FFI behavior are affected.

## Persistence & Migration Rules

- `src-tauri/src/db/schema.sql` is the canonical schema and is included by `crates/stereodrome-core/src/db.rs` for both desktop and mobile runtimes.
- If the schema changes, update `run_migrations` in `crates/stereodrome-core/src/db.rs` for existing user databases.
- For persisted settings in `src-tauri/src/commands/settings.rs`, add serde defaults for new fields and clamp user input in setters.
- For shared settings and persisted runtime state in `crates/stereodrome-core`, add backward-compatible defaults/clamping and migrations as appropriate; both desktop and mobile use this state.

## Event & Capability Coupling

- `CoreEvent` and `CoreSnapshot` are cross-platform contracts. If they change, regenerate protocol types and update the desktop event bridge, mobile client/store, and native media-session projections as affected.
- Desktop runtime snapshots are projected to Tauri events in `src-tauri/src/runtime.rs`. If changing a projected event name or payload, update matching Svelte listeners in the same change.
- If using new Tauri APIs/plugins/permissions, update `src-tauri/capabilities/*.json` with least-privilege access.
- Mobile receives the ordered runtime event stream through the FFI callback and Expo `core-event`. If its wire format or native playback projection changes, update Rust, Swift, Kotlin, and React Native consumers together.
