# Stereodrome - Project Overview

## Purpose
Stereodrome is a cross-platform desktop music player for Subsonic-compatible music servers.

**Key Features:**
- Sleek interface inspired by classic iTunes, modernized
- Local metadata storage for faster interface and song searching
- Local SQLite database for song metadata
- Tantivy for full-text search
- Rodio as audio backend

## Tech Stack

### Frontend
- **Framework:** Svelte 5 (with new runes syntax)
- **Build Tool:** Vite 6
- **Package Manager:** Bun
- **Meta-Framework:** SvelteKit (SPA mode with adapter-static)
- **UI Library:** DaisyUI
- **Language:** TypeScript (strict mode)

### Backend
- **Framework:** Tauri 2
- **Language:** Rust (edition 2021)
- **Database:** SQLite (for metadata storage)
- **Search:** Tantivy (full-text search)
- **Audio:** Rodio

### Development Environment
- **Platform:** macOS (Darwin 23.6.0)
- **Version Control:** Git

## Project Identifier
- App ID: `dev.xikxp1.stereodrome`
- Version: 0.1.0
- License: MIT
