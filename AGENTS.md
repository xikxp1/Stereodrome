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
  - `vp run rust:check`
- If a change crosses desktop, mobile, and/or shared Rust boundaries, run the checks for every affected area.

## Project Conventions

- Root desktop frontend is SPA-only (`src/routes/+layout.ts` has `ssr = false`): avoid SSR-only patterns.
- Use Svelte 5 runes (`$state`, `$derived`, `$effect`, `$props`) for new root Svelte stateful frontend code.
- Mobile UI is Expo/React Native. Keep mobile UI and platform integration inside `mobile` unless the change is intentionally shared through Rust FFI or shared TypeScript abstractions.
- Keep Rust/TypeScript payload fields in `snake_case` unless explicit serde renames are added.
- Use structured logging (`log` crate / Tauri log plugin / platform logging), not `println!` or `console.log`.

## Tauri Command Checklist

When adding, removing, or renaming a Tauri command, update all of these:

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
