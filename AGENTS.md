# AGENTS.md

## Purpose

This file is project-specific customization for AI coding agents working inside the Aria repository.

Aria is a desktop-first classical music player with Android and Android Auto support:

- a Rust service core (`/crates`)
- a Tauri 2 shell (`/src-tauri`) plus custom Android Kotlin/JNI services
- a React + TypeScript frontend (`/src`)
- a local- and remote-library workflow where tags, playback quality, and classical metadata are first-class concerns

The design target is a polished native music product, not a generic web dashboard in a web view.

## Product Snapshot

Aria currently has these top-level panes:

- `Library`: album browser with cover art and local/remote filtering
- `Album`: selected-album detail view, tag editing/exporting, and remote upload actions
- `Tracks`: grouped track browser with configurable per-pane columns, sorting, and tag inspection/editing
- `Player`: focused now-playing view with artwork, playback metadata, and raw-tag inspector
- `Playlists`: persistent playlists with collage artwork, M3U/M3U8/PLS import, M3U export, and remote playlist sync
- `Queue`: persistent playback queue
- `Settings`: library directories, per-format database field mappings, catalog rules, playback preferences, remote storage targets/cache, and diagnostic logs

Current product capabilities include:

- local-library scanning into SQLite with raw tags + per-format mapped fields
- configurable per-format database field mappings (`DEFAULT`, `FLAC`, `MP3`, `MP4`, `AAC`, `OGG`, `OPUS`, `WAV`, `AIFF`)
- configurable composer-aware catalog parsing rules
- in-place tag editing and mapped-field export (FLAC/Vorbis, ID3v2, MP4 `ilst`)
- embedded FLAC and MP4/M4A cover extraction + sidecar fallback
- remote storage sync and playback caching (Google Drive, WebDAV, SMB, Filesystem)
- playlist and queue persistence
- Windows shared and WASAPI exclusive playback, macOS/Linux shared playback, and Android USB DAC bit-perfect / Bluetooth HD / Android Auto playback
- structured diagnostic logging

## Architecture Guardrails

- Keep the frontend presentation-focused.
- Keep `src-tauri` thin; register IPC commands and delegate business logic to `crates/app-core`.
- Put business logic in Rust crates under `/crates`.
- Treat playback as a long-lived service, not a set of ad hoc command handlers.
- Do not let real-time audio paths depend on SQLite, artwork decoding, or heavy locks.
- Model classical metadata explicitly. Do not collapse everything into album/artist/track.
- Never return plaintext remote credentials or OAuth tokens to the WebView; always redact secrets across IPC and preserve stored secrets on update.
- Keep Tauri's `assetProtocol.scope` restricted to app-data/cache directories plus dynamically authorized library roots (never `**`).
- On Android, never block the main UI thread in `AriaPlaybackService` or `MainActivity` (offload Rust/SQLite initialization to a background executor), always call `startForeground()` on foreground service start before stopping/detaching, and preserve the custom Kotlin sources under `src-tauri/gen/android/app/src/main/java/com/aria/player/`.

## Repository Shape

Current top-level ownership is:

- `/src`: React UI and pane-specific presentation logic
- `/src-tauri`: Tauri command registration, desktop/mobile shell integration, `auto_jni.rs`, and checked-in Android Kotlin sources
- `/crates/domain`: shared DTOs, default mapping/catalog presets, and diagnostic logging
- `/crates/app-core`: orchestration across services, playlist import (`playlist_import.rs`), remote secret redaction (`remote_secrets.rs`), Android Auto models (`automotive.rs`)
- `/crates/library`: scan pipeline, raw tags, per-format mapping, catalog extraction, tag writing, artwork caching, and remote manifest generation (`manifest.rs`)
- `/crates/playback`: playback service, queue, `rubato` resampler, Windows WASAPI exclusive output, and Android bit-perfect output
- `/crates/playlists`: playlist service, mutations, and automatic `Favorites` playlist
- `/crates/remote-storage`: remote backends (`gdrive`, `webdav`, `smb`, `filesystem`), upload/sync pipeline, quota evaluation, LRU cache, and prefetcher
- `/crates/storage`: SQLite persistence, migrations, and settings store
- `/crates/artwork`, `/crates/search`, `/crates/platform-audio`: reserved workspace stub crates

If you add a new directory or module, keep ownership boundaries obvious.

## Default Technical Choices

Unless there is a strong reason to change course:

- Frontend: React + TypeScript + Vite
- Backend: Rust + Tokio
- Persistence: SQLite
- IPC style: Tauri commands for request/response, app events (`aria://app-event`) for streamed state

## Metadata Rules

- Preserve raw file tags separately from normalized Aria fields.
- Support multi-value credits and roles.
- Keep composer, work, movement, performer, ensemble, and conductor as first-class concepts.
- Preserve MP4 freeform `mean` casing (`----:com.apple.iTunes:*`) when normalizing, reading, and writing tags.
- Avoid encoding classical structure only in display strings.
- Do not silently discard uncommon raw tags just because they are not part of the default field list.

## Library Rules

- The library root list is part of persistent state.
- Adding a new library directory should be safe to repeat; duplicate roots should be rejected cleanly, and overlapping roots must deduplicate tracks.
- Field mappings are per-format, user-configurable, and persisted; remapping tracks must preserve synthetic remote fields (`album_path`) and remote tracks without `raw_tags`.
- Catalog extraction rules are user-configurable and persisted.
- If scan or metadata behavior changes materially, update `docs/LIBRARY.md`.

## Playback Rules

- Optimize first for correctness and predictable state flow.
- Keep WASAPI / Android exclusive paths free of unaligned byte casts, blocking calls while holding hardware render buffers, and leaked Win32/JNI handles.
- PCM-first is acceptable; treat DSD, DoP, and advanced DAC features as staged enhancements unless explicitly in scope.
- Do not mislabel shared-mode output as bit-perfect.

## UX & Frontend Performance Guardrails

- Preserve the desktop-native feel across both dark and light themes; always use semantic CSS tokens from `src/styles.css` rather than hardcoded dark hex colors.
- Avoid generic dashboard UI patterns when a music-product interaction is more appropriate.
- Keep the top-level layout dense and intentional; do not reintroduce large ornamental padding without a product reason.
- Context menus, tables, queue behavior, and playlist behavior should stay internally consistent across panes.
- Guard debounced persistence and event listeners against high-frequency playback ticks (memoize callbacks, clean up async `listenToAppEvents` subscriptions on unmount, and avoid per-cell `ResizeObserver` allocations).
- Never nest interactive elements (`<button>` inside `<button>` or interactive `role="button"` wrappers without keyboard handlers).

## Working Style For Agents

- Inspect the existing code before changing behavior.
- Prefer small vertical slices that touch one service boundary at a time.
- Prefer typed DTOs between crates and between backend and frontend.
- Add or update targeted, self-contained tests (using temporary generated fixtures — never personal machine paths) when fixing bugs or edge cases.
- Keep logs useful, but avoid noisy logs in playback loops.
- Document architecture changes in `docs/ARCHITECTURE.md` when they materially affect crate ownership or the app model.
- Update `README.md` when developer or user workflows change.

## Verification Defaults

For normal UI or integration changes, prefer these checks:

- `npm.cmd run build`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` (or targeted crate tests like `cargo test -p aria-library`)

Android/Kotlin changes are not compiled by Windows host `cargo test` and must be verified with the Android/Gradle toolchain.

## Things To Avoid

- frontend code reading the filesystem directly
- Tauri commands opening their own unmanaged database connections for one-off work
- UI state becoming the source of truth for playback position or transport
- audio-thread code waiting on async tasks unrelated to output timing
- committing machine-specific paths (`C:\Users\...`) in tests or preview fixtures
- locking the product into only album-based browsing
