# Stereodrome

Desktop and mobile music player for Subsonic-compatible servers, inspired by classic iTunes-era library browsing.


## Features

- Music playback experience inspired by legacy iTunes versions (2010-2012)
- Sync library metadata from your Subsonic server
- Full-text search powered by Tantivy
- System integration: media controls, system tray, keyboard controls
- Local audio cache with configurable size
- Mini player and desktop notifications
- Gapless playback and crossfade
- 12-band equalizer, audio normalization with dynamic compression, binaural audio

## Requirements

### Supported Servers

Stereodrome works with Subsonic API-compatible servers, including:

- [Navidrome](https://www.navidrome.org/) (recommended)
- [Airsonic](https://airsonic.github.io/)
- [Gonic](https://github.com/sentriz/gonic)
- [Subsonic](http://www.subsonic.org/)
- Other compatible servers

### Supported Platforms

- **macOS** 10.15 (Catalina) or later
- **Windows** 10 or later
- **Linux** with GTK 3
- **iOS** 15 or later
- **Android** 8 or later

## Installation

Download the latest build for your platform from [Releases](https://github.com/xikxp1/Stereodrome/releases).

## Quick Start

1. Launch Stereodrome.
2. Enter your server URL, username, and password.
3. Sync your library.
4. Start playback and adjust settings from the top bar and settings panel.

## Development

- `cargo run -p stereodrome-desktop --bin stereodrome` - Run the native desktop app
- `cargo test -p stereodrome-desktop` - Test the desktop backend and shell
- `bun install` from `mobile` - Install mobile dependencies
- `bun run rust:ios` or `bun run rust:android` from `mobile` - Cross-compile Rust for mobile
- `bun run ios` or `bun run android` from `mobile` - Build and run the mobile app

## License

MIT

## Contributing

Contributions are welcome! Please open an issue to discuss proposed changes before submitting a pull request.
