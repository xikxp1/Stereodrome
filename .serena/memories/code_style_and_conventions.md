# Code Style and Conventions

## TypeScript/Svelte

### TypeScript Configuration
- **Strict mode enabled**: All strict type checking options are on
- **ES Module Interop**: Enabled for better compatibility
- **Module Resolution**: Using "bundler" strategy
- **Source Maps**: Enabled for debugging

### Svelte 5 Conventions
- Use **new runes syntax**:
  - `$state()` for reactive state
  - `$derived()` for computed values
  - `$effect()` for side effects
  - `$props()` for component props
- Component files: `+page.svelte`, `+layout.svelte`, etc. (SvelteKit conventions)
- TypeScript in script tags: `<script lang="ts">`
- Component-scoped styles in `<style>` blocks

### File Naming
- SvelteKit routes use `+` prefix: `+page.svelte`, `+layout.ts`
- Regular components: PascalCase (e.g., `MusicPlayer.svelte`)
- TypeScript files: `.ts` extension
- Config files: lowercase with dots (e.g., `vite.config.js`)

## Rust

### General Conventions
- **Edition**: 2021
- **Formatting**: Use `cargo fmt` with default rustfmt settings (no custom config)
- **Linting**: Use `cargo clippy` for lint checks
- **Naming**: Follow standard Rust conventions
  - Snake_case for functions, variables, modules
  - PascalCase for types, structs, enums
  - SCREAMING_SNAKE_CASE for constants

### Tauri-Specific Patterns
- Tauri commands use `#[tauri::command]` attribute
- Command functions should have descriptive names
- Commands are registered in `lib.rs` via `tauri::generate_handler![]`
- Use `serde` for serialization/deserialization of data passed between frontend and backend

### Library Structure
- Main library: `lib.rs` exports the `run()` function
- Binary: `main.rs` calls `stereodrome_lib::run()`
- Crate type: `["staticlib", "cdylib", "rlib"]` for Tauri compatibility

## General Practices

### No Linting/Formatting Config
- No ESLint or Prettier configuration files
- Rely on editor defaults and TypeScript strict mode
- For Rust: use default `cargo fmt` and `cargo clippy` settings

### SPA Architecture
- Using `@sveltejs/adapter-static` with fallback to `index.html`
- No server-side rendering (Tauri doesn't support SSR)
- Client-side routing handled by SvelteKit

### Comments
- Rust: Use doc comments (`///`) for public APIs
- TypeScript/Svelte: Use JSDoc comments for complex functions
- Inline comments for non-obvious logic
