# AGENTS.md

## Dependency Management

- Backend dependencies: use `cargo add`.
- Frontend dependencies: use `bun add`.
- Do not use `npm`.
- After dependency changes, run `bun install` to refresh `bun.lock`.

## Validation

- Run checks after making code changes.
- Frontend checks:
  - `bun run check`
  - `bun run lint`
  - `bun run format:check`
- Backend checks (from `src-tauri`):
  - `cargo fmt --check`
  - `cargo clippy -- -D warnings`
- If both frontend and backend are changed, run all checks above.

## Project Conventions

- Frontend is SPA-only (`src/routes/+layout.ts` has `ssr = false`): avoid SSR-only patterns.
- Use Svelte 5 runes (`$state`, `$derived`, `$effect`, `$props`) for new stateful frontend code.
- Keep Rust/TypeScript payload fields in `snake_case` unless explicit serde renames are added.
- Use structured logging (`log` crate / Tauri log plugin), not `println!` or `console.log`.

## Tauri Command Checklist

When adding, removing, or renaming a Tauri command, update all of these:

1. Implement/update the Rust command in `src-tauri/src/commands/*.rs` with `#[tauri::command]`.
2. Re-export it in `src-tauri/src/commands/mod.rs`.
3. Register it in `src-tauri/src/lib.rs` under `tauri::generate_handler!`.
4. Add/update the frontend wrapper in `src/lib/api/commands.ts`.
5. Add/update TypeScript types in `src/lib/types/index.ts` if payload/response shapes changed.

## Persistence & Migration Rules

- If `src-tauri/src/db/schema.sql` changes, update migration handling in `src-tauri/src/db/mod.rs` (`run_migrations`) for existing user DBs.
- For persisted settings in `src-tauri/src/commands/settings.rs`, add serde defaults for new fields and clamp user input in setters.

## Event & Capability Coupling

- Playback and queue events are cross-layer contracts. If changing emit names/payloads in Rust, update matching listeners in Svelte stores/services in the same change.
- If using new Tauri APIs/plugins/permissions, update `src-tauri/capabilities/*.json` with least-privilege access.
