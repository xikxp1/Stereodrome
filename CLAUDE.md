# Stereodrome

Desktop music player for Subsonic-compatible music servers. Cross-platform app with local metadata caching.

## Tech Stack

- **Frontend:** Svelte 5, SvelteKit (SPA mode), TypeScript, DaisyUI
- **Backend:** Tauri 2, Rust
- **Storage:** SQLite (metadata), Tantivy (search)
- **Audio backend:** Rodio
- **Package Manager:** Bun + Cargo

Consult @docs/PLAN.md before every task. Update @docs/PLAN.md after every change to keep it up-to-date.

## Project Structure

```
src/                 # Svelte frontend (routes, components)
src-tauri/
├── src/
│   ├── lib.rs      # Tauri commands and app setup
│   └── main.rs     # Entry point
├── Cargo.toml      # Rust dependencies
└── tauri.conf.json # App configuration
```

## Commands

**Development**
- `bun run tauri dev` - Run full app (frontend + backend)
- `bun run check` - Type-check Svelte components
- `cd src-tauri && cargo fmt && cargo clippy` - Format and lint Rust

**Build**
- `bun run tauri build` - Create production bundle

Always run `bun run check` for frontend changes. Always run `cargo fmt && cargo clippy` in `src-tauri/` for Rust changes.

Use `bun run tauri dev` to verify changes work in the running app.

## Coding Notes

**Svelte 5:** Use new runes syntax (`$state()`, `$derived()`, `$effect()`, `$props()`)

**Tauri Commands:**
- Define in `src-tauri/src/lib.rs` with `#[tauri::command]`
- Register via `tauri::generate_handler![command_name]`
- Call from frontend: `import { invoke } from "@tauri-apps/api/core"` then `invoke("command_name", { args })`

**Frontend-Backend:**
- Frontend calls Rust via `invoke()` function
- Use `serde` for serializing data between layers
- App runs as SPA (no SSR)

Use frontend design skill when designing UI components.

Use code simplifier skill after initial implementation.
