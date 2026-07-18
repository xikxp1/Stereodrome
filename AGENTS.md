# AGENTS.md

## Repository Layout

- Native GPUI desktop app: `crates/stereodrome-desktop`.
- Shared Rust crates: `crates/stereodrome-audio`, `crates/stereodrome-core`, `crates/stereodrome-ffi`.
- Mobile app: Expo/React Native app in `mobile`.
- Mobile native module bridge: `mobile/modules/stereodrome-core`, backed by `crates/stereodrome-ffi`.

## Dependency Management

- Rust dependencies: use `cargo add` from the workspace root, or pass `-p <crate>` for a specific crate.
- Mobile dependencies: use `bun add` from `mobile`.
- Do not use `npm`.
- After mobile dependency changes, run `bun install` from `mobile` to refresh `mobile/bun.lock`.

## Validation

- Run checks after making code changes.
- Native desktop checks:
  - `cargo fmt --check`
  - `cargo clippy -p stereodrome-desktop --all-targets -- -D warnings`
  - `cargo test -p stereodrome-desktop`
- Mobile JS checks (from `mobile`):
  - `bun run typecheck`
  - `bun run lint`
- Shared/mobile Rust checks:
  - `cargo fmt --check`
  - `cargo clippy -p stereodrome-core -p stereodrome-ffi -- -D warnings`
  - `cargo test -p stereodrome-core -p stereodrome-ffi`
- Mobile native bridge checks (from `mobile`) when changing `crates/stereodrome-ffi`, `crates/stereodrome-core`, `mobile/modules/stereodrome-core`, or generated native library artifacts:
  - `bun run rust:check`
- If a change crosses desktop, mobile, and/or shared Rust boundaries, run the checks for every affected area.

## Project Conventions

- Keep desktop UI and platform services in `crates/stereodrome-desktop/src/ui`.
- Keep backend operations independent of GPUI types and expose them through the shared `DesktopModel`.
- Use typed GPUI actions and `DesktopEvent` values instead of string event names.
- Mobile UI is Expo/React Native. Keep mobile UI and platform integration inside `mobile` unless the change is intentionally shared through Rust FFI.
- Keep Rust/TypeScript payload fields in `snake_case` unless explicit serde renames are added.
- Use structured logging (`log` crate / platform logging), not `println!` or `console.log`.

## Desktop Operation Checklist

When adding, removing, or renaming a desktop operation:

1. Implement shared business logic in `crates/stereodrome-desktop/src/operations` or `crates/stereodrome-core`.
2. Update `DesktopModel` state and typed actions/events.
3. Update every GPUI view and native-service caller.
4. Add or update observable contract coverage.

## Mobile FFI Checklist

When adding, removing, or renaming a mobile FFI operation, update all affected layers:

1. Implement/update Rust dispatch and types in `crates/stereodrome-ffi`.
2. Keep shared business logic in `crates/stereodrome-core` where it is needed by both desktop and mobile.
3. Update native module bindings in `mobile/modules/stereodrome-core` for iOS and Android when exported symbols or native bridge behavior changes.
4. Update the mobile TypeScript wrapper/types in `mobile` for payload/response shape changes.
5. Run `bun run rust:check` from `mobile` when native artifacts or FFI behavior are affected.

## Persistence & Migration Rules

- Desktop schema and migrations live in `crates/stereodrome-core`.
- Add serde defaults for new persisted settings and clamp user input in setters.
- Preserve compatibility with the installed profile at `dev.xikxp1.stereodrome`.
- If shared/mobile persistence changes, update migrations/defaults for desktop and mobile FFI callers.

## Event Coupling

- Playback and queue events are cross-layer contracts. Update matching GPUI model projections and native services in the same change.
- Mobile playback, sync, offline, and queue payloads are FFI contracts. Update mobile TypeScript callers and native bridge expectations in the same change.
