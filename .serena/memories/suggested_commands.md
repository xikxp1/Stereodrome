# Suggested Commands

## Development

### Frontend Development
- `bun run dev` - Start Vite dev server (port 1420)
- `bun run build` - Build frontend for production
- `bun run preview` - Preview production build
- `bun run check` - Type-check Svelte components with svelte-check
- `bun run check:watch` - Type-check in watch mode

### Full App Development (Tauri)
- `bun run tauri dev` - Run full Tauri app in development mode (runs both frontend and backend)
- `bun run tauri build` - Build production application bundle

### Rust Backend (run from `src-tauri/` directory)
- `cargo build` - Build Rust code
- `cargo build --release` - Build optimized Rust code
- `cargo test` - Run Rust tests
- `cargo fmt` - Format Rust code
- `cargo clippy` - Run Rust linter

## Package Management

### Frontend
- `bun install` - Install npm dependencies
- `bun add <package>` - Add a dependency
- `bun add -D <package>` - Add a dev dependency

### Backend
- `cargo add <crate>` - Add a Rust dependency (in src-tauri/)
- `cargo update` - Update Rust dependencies

## System Commands (macOS)

Standard macOS/Unix commands:
- `git status`, `git add`, `git commit`, `git push` - Version control
- `ls`, `cd`, `pwd` - Directory navigation
- `grep`, `find` - File searching
- `cat`, `less` - File viewing

## Running the Application

**Development Mode:**
```bash
bun run tauri dev
```
This command:
1. Runs `bun run dev` (starts Vite dev server on port 1420)
2. Builds and runs the Rust backend
3. Opens the app window

**Production Build:**
```bash
bun run tauri build
```
Creates a distributable application bundle in `src-tauri/target/release/bundle/`
