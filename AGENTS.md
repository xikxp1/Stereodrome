# AGENTS.md

## Repository Layout

- Root frontend: SvelteKit SPA used by the Tauri desktop app.
- Desktop shell/backend: `src-tauri`.
- Shared Rust workspace crates: `crates/stereodrome-audio`, `crates/stereodrome-core`, `crates/stereodrome-ffi`.
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
- Desktop/Tauri Rust checks:
  - `cargo fmt --check`
  - `cargo clippy -p stereodrome -- -D warnings`
- Shared/mobile Rust checks:
  - `cargo fmt --check`
  - `cargo clippy -p stereodrome-core -p stereodrome-ffi -- -D warnings`
  - `cargo test -p stereodrome-core -p stereodrome-ffi`
- Mobile native bridge checks (from `mobile`) when changing `crates/stereodrome-ffi`, `crates/stereodrome-core`, `mobile/modules/stereodrome-core`, or generated native library artifacts:
- If a change crosses desktop, mobile, and/or shared Rust boundaries, run the checks for every affected area.

## Generated Protocol Types

- `src/lib/types/protocol.generated.ts` and `mobile/src/core/protocol.generated.ts` are generated from the Rust types by `scripts/generate-protocol-types.sh`. Never hand-edit them.
- After changing any type in `crates/stereodrome-core/src/{models,queue,lastfm,backup,protocol}.rs`, run `vp run protocol:types` and commit the result. CI fails via `vp run protocol:types:check` when the output is stale.
- To expose a new type, derive `TS` with `#[cfg_attr(feature = "ts", derive(ts_rs::TS))]` and add it to the list in `crates/stereodrome-core/src/bin/export-protocol-types.rs`.
- Command result payloads cannot be derived: the runtime erases them to `serde_json::Value`. The command-to-payload mapping is the hand-maintained `COMMAND_RESULTS` table in `crates/stereodrome-core/src/bin/export-protocol-types.rs`, which generates `CoreCommandValue` for both platforms. Keep it in sync with `runtime/effect.rs` and `runtime/mod.rs`; commands omitted from it resolve to `void` on the client.

## Project Conventions

- Root desktop frontend is SPA-only (`src/routes/+layout.ts` has `ssr = false`): avoid SSR-only patterns.
- Use Svelte 5 runes (`$state`, `$derived`, `$effect`, `$props`) for new root Svelte stateful frontend code.
- Mobile UI is Expo/React Native. Keep mobile UI and platform integration inside `mobile` unless the change is intentionally shared through Rust FFI or shared TypeScript abstractions.
- Keep Rust/TypeScript payload fields in `snake_case` unless explicit serde renames are added. Prefer changing the frontend over adding a serde rename, since renames force hand-written wrapper structs that bypass the generated types.
- Use structured logging (`log` crate / Tauri log plugin / platform logging), not `println!` or `console.log`.

## Desktop Runtime Commands

Runtime operations reach the desktop through the single `core_dispatch` Tauri command, mirroring how mobile dispatches over FFI. Prefer this path:

- Call `dispatch({ type: "..." })` from `src/lib/api/core.ts`. The payload type comes from the generated `CoreCommandValue`, so no hand-written return type is needed.
- A new `CoreCommand` variant needs no desktop Rust change at all. If it returns a value, add it to `COMMAND_RESULTS` in `crates/stereodrome-core/src/bin/export-protocol-types.rs` and re-run `vp run protocol:types`. The exporter panics if the tag is not a real variant.

Only add a dedicated `#[tauri::command]` when the operation needs desktop-specific work (keyring, windowing, tray, file I/O, desktop settings store, event emission). In that case update all of these:

1. Implement/update the Rust command in `src-tauri/src/commands/*.rs` with `#[tauri::command]`.
2. Re-export it in `src-tauri/src/commands/mod.rs`.
3. Register it in `src-tauri/src/lib.rs` under `tauri::generate_handler!`.
4. Add/update the frontend wrapper in `src/lib/api/commands.ts`.
5. Add/update TypeScript types in `src/lib/types/index.ts` if payload/response shapes changed.

## Mobile FFI Checklist

When adding, removing, or renaming a mobile FFI operation, update all affected layers:

1. Implement/update Rust dispatch and types in `crates/stereodrome-ffi`.
2. Keep shared business logic in `crates/stereodrome-core` where it is needed by both desktop and mobile.
3. Update native module bindings in `mobile/modules/stereodrome-core` for iOS and Android when exported symbols or native bridge behavior changes.
4. Update the mobile TypeScript wrapper/types in `mobile` for payload/response shape changes.
5. Run `vp run rust:check` from `mobile` when native artifacts or FFI behavior are affected.

## Persistence & Migration Rules

- If `src-tauri/src/db/schema.sql` changes, update migration handling in `src-tauri/src/db/mod.rs` (`run_migrations`) for existing user DBs.
- For persisted settings in `src-tauri/src/commands/settings.rs`, add serde defaults for new fields and clamp user input in setters.
- If shared/mobile persistence changes in `crates/stereodrome-core`, update migrations/defaults for both desktop callers and mobile FFI callers.

## Event & Capability Coupling

- Playback and queue events are cross-layer contracts. If changing emit names/payloads in Rust, update matching listeners in Svelte stores/services in the same change.
- If using new Tauri APIs/plugins/permissions, update `src-tauri/capabilities/*.json` with least-privilege access.
- Mobile playback, sync, offline, and queue payloads are FFI contracts. If changing Rust payloads or method names, update the mobile TypeScript callers and native bridge expectations in the same change.
