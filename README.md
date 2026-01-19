# Stereodrome

Stereodrome is a desktop music player for Subsonic-compatible music servers inspired by iTunes.

## Features

- Cross-platform (macOS, Windows, Linux) via Tauri 2
- iTunes-inspired interface with Svelte 5 and DaisyUI
- Local metadata caching with SQLite for fast browsing
- Full-text search powered by Tantivy
- Rodio audio backend

## Tech Stack

- **Frontend:** Svelte 5, SvelteKit, TypeScript, DaisyUI
- **Backend:** Tauri 2, Rust, SQLite, Tantivy, Rodio

## Getting Started

**Prerequisites:** [Bun](https://bun.sh/) and [Rust](https://rustup.rs/)

```bash
# Install dependencies
bun install

# Run in development mode
bun run tauri dev

# Build for production
bun run tauri build
```

## Commands

| Command | Description |
|---------|-------------|
| `bun run tauri dev` | Run app in development mode |
| `bun run tauri build` | Build production bundle |
| `bun run check` | Type-check Svelte components |

Always use `bun` instead of `npm`.

## Project Structure

```
src/           # Svelte frontend
src-tauri/     # Rust backend (Tauri commands in src/lib.rs)
static/        # Static assets
docs/          # Documentation
```

## License

MIT
