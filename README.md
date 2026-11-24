# Aria

Aria is a desktop-first classical music player with Android and Android Auto support, built around classical-music metadata, bit-accurate playback, and user-owned local or remote libraries.

## Highlights

- **Local & remote classical libraries:** Scan local music folders into SQLite and optionally sync albums and playlists to/from your own **Google Drive**, **WebDAV**, **SMB**, or local/filesystem remote storage targets with a sliding-window prefetch cache.
- **Per-format tag mapping & raw tag preservation:** Preserves raw tags separately from normalized database fields (`composer`, `work`/title, `catalog`, `conductor`, `ensemble`, `soloist`, `album`, `year`, `disk_number`, `track_number`, `genre`), with customizable per-format priority profiles (`DEFAULT`, `FLAC`, `MP3`, `MP4`, `AAC`, `OGG`, `OPUS`, `WAV`, `AIFF`).
- **Composer-aware catalog extraction:** Automatically parses classical catalog numbers (`BWV`, `WAB`, `K`, `KV`, `D`, `RV`, `HWV`, `TWV`, `BuxWV`, `Hob.`, `S.`, `WoO`, `Op`) with colon-segment prioritization, range suppression, and Unicode diacritic folding.
- **In-place tag editing & field export:** Inspect all raw tags on a track, edit tags across single or multiple tracks, and export mapped fields back into audio files (FLAC/Vorbis, ID3v2, MP4 `ilst`).
- **Artwork resolution:** Extracts embedded cover art from FLAC and MP4/M4A files (cached locally) and falls back to sidecar images (`cover`, `folder`, `front`).
- **High-resolution playback:** Shared output via `rodio`, Windows **WASAPI exclusive mode**, **Android USB DAC bit-perfect** (`MIXER_BEHAVIOR_BIT_PERFECT` on Android 14+) and **Bluetooth HD audio**, plus high-quality `rubato` resampling when hardware requires rate conversion.
- **Multi-pane desktop & mobile UI:** `Library`, `Album`, `Tracks`, `Player`, `Playlists` (with M3U/M3U8/PLS import & M3U export), `Queue`, and `Settings` (including `Diagnostic logs`).
- **Android Auto support:** Native media browser and playback service (`AriaPlaybackService`) exposing library browsing and transport controls.

## Tech Stack

- **Shell:** `Tauri 2` (Windows, macOS, Linux, Android)
- **Frontend:** `React 18 + TypeScript + Vite`
- **Backend:** `Rust + Tokio`
- **Persistence:** `SQLite` (`rusqlite`)
- **Audio & Tagging:** `symphonia`, `rodio`, `rubato`, `lofty`, Windows WASAPI (`windows` crate), Android `AudioTrack` / `MediaBrowserServiceCompat` (Kotlin + JNI)

The codebase is organized as a Rust workspace with presentation-focused UI code in `/src`, a thin Tauri/JNI shell in `/src-tauri`, and business logic in `/crates`.

## Repository Map

- `src/`: React + TypeScript frontend (`app/`, `components/`, `features/{albums,library,now-playing,playback,playlists,queue,remote,settings,tracks}`, `lib/`, `types/`)
- `src-tauri/`: Tauri desktop/mobile shell, IPC commands (`src/commands/`), Android Auto JNI bridge (`src/auto_jni.rs`), and custom checked-in Android Kotlin code (`gen/android/app/src/main/java/com/aria/player/`)
- `crates/domain/`: Shared serializable DTOs, default mapping profiles, catalog rules, and diagnostic logging
- `crates/app-core/`: Service orchestration, playlist import matching, remote secret redaction, background workers, and Android Auto view models
- `crates/library/`: Recursive scanning, raw tag extraction, per-format field mapping, catalog extraction, in-place tag writing, artwork caching, and remote manifest builders
- `crates/playback/`: Long-lived playback service, persistent queue, resampling (`rubato`), Windows WASAPI exclusive output, and Android bit-perfect output
- `crates/playlists/`: Persistent playlist service and automatic `Favorites` playlist
- `crates/remote-storage/`: Remote storage backends (`gdrive`, `webdav`, `smb`, `filesystem`), upload/sync pipelines, quota checks, sliding-window cache, and prefetch worker
- `crates/storage/`: SQLite persistence, schema migrations, and settings store
- `crates/artwork/`, `crates/search/`, `crates/platform-audio/`: Reserved workspace support crates (currently minimal stubs; see [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md))
- `docs/`: Architecture, library pipeline, playback, remote storage, and Android documentation
- `AGENTS.md`: Project guardrails and workflow defaults for AI coding agents

## Platform Support

| Platform | Status | Notes |
| --- | --- | --- |
| Windows (x64) | Primary desktop target | Shared audio + WASAPI exclusive mode; `.msi` and NSIS `.exe` installers |
| macOS (Universal) | Desktop build | Shared audio; native AppleScript/Finder file dialogs |
| Linux (x64) | Desktop build | Shared audio (ALSA); `zenity`/`kdialog`/`xdg-open` file dialogs |
| Android & Android Auto | Supported mobile target | Native `AudioTrack` USB DAC bit-perfect & Bluetooth HD paths, foreground media service; see [docs/ANDROID.md](./docs/ANDROID.md) |

## Prerequisites

Install these on your machine and make sure they are available on `PATH`:

- Node.js (LTS) and npm
- Rust (stable) and Cargo
- Tauri OS prerequisites for your platform (WebView2 on Windows; `libwebkit2gtk-4.1-dev`, `libasound2-dev`, etc. on Linux)
- For Android builds: Android SDK, NDK, and Rust Android targets (see [docs/ANDROID.md](./docs/ANDROID.md))

Windows notes:

- In PowerShell, `npm.cmd` is often more reliable than `npm` if PowerShell script execution policy is restricted.

## Developer Workflow

Install dependencies:

```powershell
npm.cmd install
```

Run the desktop app in development mode:

```powershell
npm.cmd run tauri -- dev
```

Verification commands:

```powershell
# Frontend TypeScript check + production bundle
npm.cmd run build

# Rust compile check for the Tauri shell
cargo check --manifest-path src-tauri/Cargo.toml

# Workspace lints (CI enforces -D warnings)
cargo clippy --workspace --all-targets -- -D warnings

# Workspace unit and integration tests
cargo test --workspace
```

## Packaging And Release Builds

Build an optimized desktop executable and platform installer bundles:

```powershell
npm.cmd run tauri -- build
```

Typical Windows outputs:

- Release executable: `target\release\aria.exe`
- Installer bundles: `target\release\bundle\msi\` and `target\release\bundle\nsis\`

Automated multi-platform desktop releases are built by [`.github/workflows/release.yml`](./.github/workflows/release.yml) when pushing a `v*` tag (for example `v0.5.0`). CI checks run on every push and pull request via [`.github/workflows/ci.yml`](./.github/workflows/ci.yml).

## First-Use Flow

1. Open `Settings` -> `Library`
2. Click `Add directory` and pick a music folder (Aria scans automatically) — or configure a remote storage target under `Settings` -> `Remote Storage`
3. Customize `Database fields` (per audio format) and `Catalog rules` if needed
4. Select an output device and exclusive/bit-perfect preference in `Settings` -> `Playback`

## Data And Storage

Aria stores persistent library metadata, playlists, queue state, settings, and remote sync state in SQLite:

- **Windows:** `%LOCALAPPDATA%\Aria\aria.sqlite3`
- **macOS / Linux:** Platform data directory under `Aria/aria.sqlite3`
- **Android:** App internal files directory (`/data/data/com.aria.player/files/aria.sqlite3`)

Extracted embedded artwork and cached remote audio files are stored under Aria's local app-data/cache directory.

## Documentation

- [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) — Crate boundaries, threading, IPC, and security model
- [docs/LIBRARY.md](./docs/LIBRARY.md) — Library scanning, per-format field mappings, catalog rules, artwork, and tag writing
- [docs/PLAYBACK.md](./docs/PLAYBACK.md) — Playback engine, WASAPI exclusive mode, resampling, and Android bit-perfect audio
- [docs/REMOTE_STORAGE.md](./docs/REMOTE_STORAGE.md) — Remote backends, manifests, OAuth setup, caching, and sync
- [docs/ANDROID.md](./docs/ANDROID.md) — Android & Android Auto build, custom Kotlin layer, 16 KB page alignment, and signing
- [AGENTS.md](./AGENTS.md) — Guidelines for AI coding agents

## License

Aria is licensed under the Apache License, Version 2.0. See [LICENSE](./LICENSE).

## Note

This app is vibe-coded using OpenAI's Codex with GPT-5.4 on Extra High reasoning. I've had the idea of a music player more suited for classical music (at least in my mind) for a while. I recently got my hands on Codex and decided to try vibe-coding as a learning experience. GPT came up with a few tech-stack options at the beginning. I made the choice because I don't know any of the technologies, perfect for a vibe-coding exercise.
