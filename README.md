# Stereodrome

A desktop music player for Subsonic-compatible music servers, inspired by the classic iTunes interface.

## Features

- Music playback experience inspired by legacy iTunes versions (2010-2012)
- Sync library metadata from your Subsonic server
- Full-text search powered by Tantivy
- System integration: media controls, system tray, keyboard controls
- Local audio cache with configurable size

## Screenshots

*Coming soon*

## Requirements

### Subsonic Server
Stereodrome works with any Subsonic-compatible server:
- [Navidrome](https://www.navidrome.org/) (recommended)
- [Airsonic](https://airsonic.github.io/)
- [Gonic](https://github.com/sentriz/gonic)
- [Subsonic](http://www.subsonic.org/)
- Other Subsonic API-compatible servers

### System Requirements
- **macOS** 10.15 (Catalina) or later
- **Windows** 10 or later
- **Linux** with GTK 3 and WebKit2GTK

## Installation

### From Release (Recommended)
Download the latest release for your platform from the [Releases](https://github.com/xikxp1/Stereodrome/releases) page.

### Build from Source

**Prerequisites:** [Bun](https://bun.sh/) and [Rust](https://rustup.rs/)

```bash
# Clone the repository
git clone https://github.com/xikxp1/Stereodrome.git
cd Stereodrome

# Install dependencies
bun install

# Run in development mode
bun run tauri dev

# Build for production
bun run tauri build
```

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Space` | Play / Pause |
| `↑` / `↓` | Navigate songs |
| `Enter` | Play selected song |
| `Shift + ←` / `→` | Seek backward / forward |
| `Cmd/Ctrl + ↑` / `↓` | Volume up / down |
| `Cmd/Ctrl + ←` / `→` | Previous / Next track |
| `M` | Mute / Unmute |
| `S` | Cycle shuffle mode |
| `R` | Cycle repeat mode |
| `Q` | Toggle queue panel |
| `V` | Toggle visualizer |
| `Cmd/Ctrl + K` | Focus search |
| `Cmd/Ctrl + ,` | Open settings |

## Tech Stack

- **Frontend:** Svelte 5, SvelteKit, TypeScript, TanStack Query, TanStack Virtual, DaisyUI, Tailwind
- **Backend:** Tauri 2, Rust, SQLite, Tantivy, Rodio

## Known Limitations

- Incremental library sync not yet implemented (full sync only)
- No crossfade between tracks
- No gapless playback
- Credentials stored in plain text (use at your own discretion on shared machines)

## License

MIT

## Contributing

Contributions are welcome! Please open an issue to discuss proposed changes before submitting a pull request.
