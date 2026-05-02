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

## Packaging And Release Builds

Aria can be built in two release forms:

- a standalone optimized executable for local testing
- an installer package for sharing with other users

Build both with:

```powershell
npm.cmd run tauri -- build
```

What this command does:

- runs the frontend production build
- compiles the Rust app in release mode
- creates installer bundle artifacts through Tauri

Typical Windows outputs:

- release executable: `target\release\aria.exe`
- installer bundles: `target\release\bundle\`

The exact installer filenames depend on the host toolchain and Tauri bundler, but on Windows you should expect artifacts such as:

- `msi\Aria_0.1.0_x64_en-US.msi`
- `nsis\Aria_0.1.0_x64-setup.exe`

If you only want to test the optimized app locally, you can launch the release executable directly. If you want something to hand to another user, use the installer from the `bundle` directory.

On a Windows machine, the first installer build may download the WiX and NSIS tool bundles automatically. That is expected.

### Packaging Checklist

Before building an installer package, make sure:

- `npm.cmd install` has been run
- Rust/Cargo are installed and working
- Tauri Windows prerequisites are installed
- WebView2 is available on the target machine

### Clean Packaging Workflow

From a clean checkout:

```powershell
npm.cmd install
npm.cmd run tauri -- build
```

After that:

1. Check `target\release\bundle\`
2. Pick the installer format you want to distribute
3. Test-install it on the same machine or a clean Windows machine
4. Launch Aria and verify first-run flow, scanning, playback, and icon appearance

## Running Aria As A User

### From source

If you are running from a developer checkout:

```powershell
npm.cmd run tauri -- dev
```

### From a release executable

After building with `npm.cmd run tauri -- build`, launch:

```text
target\release\aria.exe
```

### From an installer package

After building with `npm.cmd run tauri -- build`, open:

```text
target\release\bundle\
```

Then run the installer file generated for Windows, such as the `.msi` or setup `.exe`.

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

- Embedded cover extraction is currently FLAC-first; other formats mainly rely on sidecar images
- The project is desktop-first; mobile is not an active primary target yet

## Related Docs

- [docs/LIBRARY.md](./docs/LIBRARY.md)
- [AGENTS.md](./AGENTS.md)

## License

Aria is licensed under the Apache License, Version 2.0. See [LICENSE](./LICENSE).

## Note

This app is vibe-coded using OpenAI's Codex with GPT-5.4 on Extra High reasoning. I've had the idea of a music player more suited for classical music (at least in my mind) for a while. I recently got my hands on Codex and decided to try vibe-coding as a learning experience. GPT came up with a few tech-stack options at the beginning. I made the choice because I don't know any of the technologies, perfect for a vibe-coding exercise. 
