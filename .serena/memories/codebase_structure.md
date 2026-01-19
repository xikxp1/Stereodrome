# Codebase Structure

## Directory Layout

```
Stereodrome/
├── src/                    # Svelte frontend source
│   ├── routes/            # SvelteKit routes
│   │   ├── +layout.ts    # Layout configuration
│   │   └── +page.svelte  # Main page component
│   └── app.html          # HTML template
├── src-tauri/             # Tauri (Rust) backend
│   ├── src/
│   │   ├── lib.rs        # Main library with Tauri commands
│   │   └── main.rs       # Application entry point
│   ├── Cargo.toml        # Rust dependencies
│   ├── Cargo.lock        # Rust dependency lock
│   ├── tauri.conf.json   # Tauri configuration
│   ├── build.rs          # Build script
│   ├── capabilities/     # Tauri capabilities/permissions
│   ├── icons/            # App icons
│   └── target/           # Rust build output (gitignored)
├── static/                # Static assets (images, etc.)
├── package.json           # Frontend dependencies & scripts
├── tsconfig.json          # TypeScript configuration
├── svelte.config.js       # Svelte/SvelteKit configuration
├── vite.config.js         # Vite configuration
└── README.md              # Project documentation
```

## Key Configuration Files

- **package.json**: Frontend dependencies and npm scripts
- **src-tauri/Cargo.toml**: Rust dependencies and package metadata
- **src-tauri/tauri.conf.json**: Tauri app configuration (window size, build commands, etc.)
- **svelte.config.js**: SvelteKit adapter configuration (SPA mode)
- **vite.config.js**: Vite dev server configuration (port 1420)
- **tsconfig.json**: TypeScript strict mode configuration

## Frontend-Backend Communication

- Frontend calls Rust commands using `@tauri-apps/api/core` `invoke()` function
- Rust commands are defined with `#[tauri::command]` macro
- Commands are registered in `lib.rs` using `tauri::generate_handler![]`
