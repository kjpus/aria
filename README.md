# Aria

Aria is a desktop-first classical music player built for local libraries and classical-music metadata.

Today it focuses on:

- local file libraries rather than streaming
- configurable tag-to-database mapping
- classical-friendly catalog number extraction
- playlists and queue management
- high-quality desktop playback, including Windows WASAPI exclusive mode where supported

## What Aria Does Today

- Scans local music directories into a persistent SQLite library
- Preserves raw tags and also builds normalized database fields
- Lets the user configure database fields and tag priorities
- Lets the user configure composer-aware catalog parsing rules
- Extracts embedded FLAC cover art first and falls back to sidecar images
- Provides Library, Album, Tracks, Playlists, Queue, and Settings panes
- Persists settings, queue state, playlists, and library scan results between launches

## Tech Stack

- `Tauri 2`
- `React + TypeScript + Vite`
- `Rust + Tokio`
- `SQLite`

The codebase is organized as a Rust workspace with UI code in `/src`, a thin Tauri shell in `/src-tauri`, and business logic in `/crates`.

## Repository Map

- `src/`: frontend UI
- `src-tauri/`: Tauri app shell and command registration
- `crates/domain/`: shared DTOs and defaults
- `crates/app-core/`: orchestration layer
- `crates/library/`: library scan and tag normalization
- `crates/playback/`: playback service and queue
- `crates/playlists/`: playlist service
- `crates/storage/`: SQLite persistence and migrations
- `docs/LIBRARY.md`: detailed library scan and metadata notes
- `AGENTS.md`: project customization for AI coding agents

## Prerequisites

Install these on your machine and make sure they are available on `PATH`:

- Node.js and npm
- Rust and Cargo
- Tauri OS prerequisites for your platform

Windows notes:

- Tauri desktop apps require WebView2
- In PowerShell, `npm.cmd` may be more reliable than `npm` if PowerShell script execution is restricted

## Developer Workflow

Install dependencies:

```powershell
npm.cmd install
```

Run the app in development mode:

```powershell
npm.cmd run tauri -- dev
```

Run a frontend production build:

```powershell
npm.cmd run build
```

Run a Rust compile check for the desktop app:

```powershell
cargo check --manifest-path src-tauri/Cargo.toml
```

## Release Build

Build an optimized desktop executable:

```powershell
npm.cmd run tauri -- build
```

Important:

- Tauri bundling is currently disabled in `src-tauri/tauri.conf.json`
- That means `tauri build` produces a release executable, but not a packaged installer

On Windows, the release executable is typically:

```text
src-tauri\target\release\aria.exe
```

## Running Aria As A User

### From source

If you are running from a developer checkout:

```powershell
npm.cmd run tauri -- dev
```

### From a release executable

After building with `npm.cmd run tauri -- build`, launch:

```text
src-tauri\target\release\aria.exe
```

### First-use flow

1. Open `Settings`
2. Click `Add directory`
3. Choose a music folder
4. Aria adds the root and starts scanning automatically
5. Adjust `Database fields` and `Catalog rules` if needed
6. Choose the playback output device in `Settings`

## Data And Storage

Aria stores its persistent state in SQLite.

On Windows, the default database path is:

```text
%LOCALAPPDATA%\Aria\aria.sqlite3
```

Aria also caches extracted embedded FLAC cover art under the local app data area.

## Library Behavior At A Glance

- Aria stores both `raw tags` and `mapped fields`
- Field mappings are priority-based per database field
- Catalog extraction is only used when the `catalog` field was not resolved from dedicated tags
- FLAC embedded cover art is preferred over sidecar cover files

For the detailed scan and mapping rules, see [docs/LIBRARY.md](./docs/LIBRARY.md).

## Current Limitations

- The main packaged-installer flow is not enabled yet
- Embedded cover extraction is currently FLAC-first; other formats mainly rely on sidecar images
- The project is desktop-first; mobile is not an active primary target yet

## Related Docs

- [docs/LIBRARY.md](./docs/LIBRARY.md)
- [AGENTS.md](./AGENTS.md)

## Note

This app is vibe-coded using OpenAI's Codex with GPT-5.4 on Extra High reasoning. I've had a idea of a music player more suited for classical music (at least in my mind) for a while. I recently got my hands on Codex and decided to try vibe-coding as a learning experience. GPT came up with a few tech-stack options at the beginning. I made the choice because I don't know any of the technologies, perfect for a vibe-coding exercise. 