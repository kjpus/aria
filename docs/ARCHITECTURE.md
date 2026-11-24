# Aria Architecture

This document outlines Aria's crate boundaries, runtime/threading model, IPC design, and security boundaries.

## High-Level Layers

```text
+------------------------------------------------------------------+
| Frontend: React 18 + TypeScript + Vite (/src)                    |
|  - Presentation panes, tables, dialogs, theming, keyboard nav    |
|  - Typed IPC wrapper (src/lib/aria.ts) + event listener          |
+------------------------------------------------------------------+
          | Tauri invoke (request/response)     ^ aria://app-event
          v                                     |
+------------------------------------------------------------------+
| Shell: Tauri 2 + Android JNI (/src-tauri)                        |
|  - Thin command handlers (src-tauri/src/commands/*)              |
|  - Dynamic AssetProtocolScope directory registration             |
|  - Android Auto JNI bridge (src-tauri/src/auto_jni.rs)           |
|  - Android Kotlin services (AriaPlaybackService, BitPerfect, BT) |
+------------------------------------------------------------------+
          |
          v
+------------------------------------------------------------------+
| Orchestration: crates/app-core                                   |
|  - AppCore coordinates Library, Playback, Playlists, Remote, DB  |
|  - Playlist import (playlist_import.rs)                          |
|  - Remote secret redaction (remote_secrets.rs)                   |
|  - Android Auto view models (automotive.rs)                      |
+------------------------------------------------------------------+
   |            |             |                 |               |
   v            v             v                 v               v
library     playback      playlists      remote-storage      storage
(lofty)  (rodio/symphonia) (in-memory)  (gdrive/webdav/smb) (rusqlite)
             |                                  |
             +-------> crates/domain <----------+
                     (DTOs, defaults, diagnostic logger)
```

## Workspace Crates

| Crate | Responsibility |
| --- | --- |
| `aria-domain` (`crates/domain`) | Shared serializable DTOs (`camelCase` JSON for TS wire compatibility), default per-format field mappings, default catalog rules, and global ring-buffered diagnostic logger. |
| `aria-library` (`crates/library`) | Local directory scanning, raw tag extraction, per-format field mapping, catalog extraction, in-place tag writing, embedded/sidecar artwork resolution, and remote manifest builders (`manifest.rs`). |
| `aria-playback` (`crates/playback`) | Long-lived playback engine, queue management, `rubato` resampling, Windows WASAPI exclusive output (`exclusive_wasapi.rs`), and Android JNI bit-perfect output (`exclusive_android.rs`). |
| `aria-playlists` (`crates/playlists`) | Persistent playlist CRUD, ordering, track deduplication, and automatic `Favorites` playlist invariants. |
| `aria-remote-storage` (`crates/remote-storage`) | Storage backend trait & implementations (`gdrive`, `webdav`, `smb`, `filesystem`), upload/sync pipelines, quota evaluation, sliding-window disk cache, and playback prefetcher. |
| `aria-storage` (`crates/storage`) | SQLite database access (`AppDatabase`), schema initialization/migrations, and `SettingsStore`. |
| `aria-app-core` (`crates/app-core`) | Top-level application service (`AppCore`) wiring all crates together, emitting `AppEvent`s, managing background workers, and handling cross-cutting workflows. |
| `aria` (`src-tauri`) | Thin Tauri command layer, platform shell helpers (`pick_directory`, `open_directory`, `show_in_explorer`, playlist file pickers), and Android JNI/Kotlin host. |

### Reserved Stub Crates

`crates/artwork`, `crates/search`, and `crates/platform-audio` are currently minimal placeholder crates reserved in the workspace for future extraction (moving artwork caching out of `crates/library`, dedicated full-text search indexing, and platform audio device enumeration).

## Threading And Tokio Runtime Model

- **Tauri shell & `AppCore`:** `src-tauri/src/lib.rs` boots a multi-threaded Tokio runtime to initialize `AppCore` and drops its runtime guard before entering `tauri::Builder::run(...)` so synchronous window/exit hooks (`AppCore::shutdown()`) never panic from calling blocking locks inside an entered runtime.
- **Playback publisher (`AppCore::spawn_playback_publisher`):** Runs on a dedicated thread with an I/O- and timer-enabled Tokio runtime (`Builder::new_current_thread().enable_all()`) polling playback snapshots, publishing `PlaybackEvent::SnapshotChanged`, and triggering async remote track buffering/prefetching.
- **Audio threads:** Windows WASAPI exclusive mode and Android bit-perfect playback run on dedicated real-time/high-priority threads that read decoded PCM from bounded channels and never block on SQLite, HTTP, or UI locks.
- **Playback shutdown:** `PlaybackService::shutdown()` is idempotent and safe to call either inside or outside a Tokio runtime context.

## IPC Contract

- **Request / Response:** Frontend calls typed helpers in `src/lib/aria.ts`, which invoke registered Tauri commands in `src-tauri/src/commands/`. All DTOs live in `crates/domain` and are mirrored in `src/types/aria.ts`.
- **Streamed Events:** Backend broadcasts `aria_domain::AppEvent` over a `tokio::sync::broadcast` channel; `src-tauri` forwards events to the WebView on the `aria://app-event` channel.
- **Browser Preview Mode:** When running outside Tauri (`npm run dev`), `src/lib/aria.ts` serves deterministic in-memory preview fixtures so UI layout and styling can be inspected in a browser.

## Security Boundaries

- **Asset Protocol Scope (`src-tauri/tauri.conf.json`):** Static scope is restricted to Aria's `$APPLOCALDATA/**`, `$APPDATA/**`, and `$CACHE/**` directories (never `**` or `$HOME/**`). Configured library root directories are dynamically granted read access via `app.asset_protocol_scope().allow_directory(root, true)` on startup and when adding a root.
- **Credential Redaction (`crates/app-core/src/remote_secrets.rs`):** Remote passwords, client secrets, and OAuth access/refresh tokens are redacted from `RemoteTarget.config_json` before crossing IPC to the WebView (`hasPassword` / `hasTokens` flags are exposed instead) and merged back from SQLite when saving target edits with blank secret fields.
- **URL & Path Validation:** External URL launches (`open_url` on desktop, `MainActivity.openBrowser` on Android) strictly enforce `http://` / `https://`. `open_directory` validates `Path::is_dir()`. WebDAV `clear_all_data` enforces same-origin and strict child-path containment under the configured base URL. OAuth callback HTML escapes error query parameters.
