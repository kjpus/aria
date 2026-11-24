# Remote Storage And Sync

Aria lets you upload albums and playlists from a desktop library to your own cloud or network storage and play them seamlessly on other desktops or Android devices using a local sliding-window cache.

## Supported Backends

Implemented in `crates/remote-storage/src/backends/`:

| Backend | `RemoteBackendType` | Notes |
| --- | --- | --- |
| Google Drive | `google_drive` | OAuth 2.0 with PKCE + loopback redirect server, automatic token refresh, folder-ID cache |
| WebDAV | `web_dav` | HTTP/HTTPS Basic Auth, `PROPFIND` directory listing & quota discovery, origin-scoped cleanup |
| SMB / Network Share | `smb` | Windows UNC share access (`WNetAddConnection2W` when credentials are supplied) |
| Local / Filesystem | `filesystem` | Local directory target (useful for external drives, synced folders, and integration tests) |

## Remote Directory & Manifest Layout

All remote targets share the same portable layout (builders live in `crates/library/src/manifest.rs`):

```text
<remote-root>/
├── aria-library-index.json          # MasterLibraryIndex (summaries of all uploaded albums)
├── aria-playlists-index.json        # MasterPlaylistIndex (summaries of all uploaded playlists)
├── playlists/
│   ├── <playlist_id>.json           # RemotePlaylistManifest
│   └── <Playlist Name>.m3u8         # Portable relative-path M3U8 export
└── <relative-album-folder>/
    ├── aria-manifest.json           # AlbumManifest (tracks, mapped fields, checksums, artwork)
    ├── cover.jpg (or cover.png)     # Album cover art (if available)
    └── <track-files>                # Audio files
```

### Index Integrity

- When uploading an album or playlist, Aria reads the existing `aria-library-index.json` / `aria-playlists-index.json`.
- An empty index is initialized **only** on `RemoteStorageError::NotFound`. Transient network, auth, rate-limit, or JSON parse errors abort the index write rather than overwriting the remote catalog with a single entry.
- Deleting an album from a remote target deletes only the tracks scoped to that album's folder prefix in SQLite (`remote_synced_items`) and removes the album from remote `aria-library-index.json`.

## Google Drive OAuth Setup

- Aria uses OAuth 2.0 Authorization Code flow with **PKCE** (`S256`) and a cryptographically random `state` parameter (via `getrandom`, failing closed if OS entropy fails).
- Built-in desktop OAuth configuration can inject `ARIA_BUILD_CLIENT_SECRET` at build time (e.g., via GitHub Actions release secrets) or users can supply their own Google Cloud OAuth Desktop client ID/secret in `Settings -> Remote Storage`.
- OAuth loopback callbacks run on `127.0.0.1` with HTML-escaped error responses and CSRF `state` verification.

## Sliding-Window Cache & Prefetching

Implemented in `crates/remote-storage/src/cache.rs` and `crates/remote-storage/src/prefetcher.rs`:

- Remote tracks use `remote://<target_id>/<relative_path>` paths in `LibrarySnapshot`.
- During playback, `PlaybackPrefetcher` downloads the active track and upcoming queue window into Aria's local cache directory using atomic `.tmp_<pid>_<nonce>` staging files that are renamed upon completion.
- Cache lookups ignore `.tmp` / `.tmp_` staging files and match exact track-ID stems (`Path::file_stem() == Some(effective_track_id)`) to avoid prefix collisions (e.g., `trk_1` vs `trk_10`).
- LRU eviction enforces the user's configured cache size limit while protecting the currently playing track and active lookahead window.

## Quota Accounting

- Per-target storage limits (`storage_limit_bytes`) are enforced before uploading new files (`crates/remote-storage/src/quota.rs`).
- `AppDatabase::get_total_synced_bytes_for_target` sums only `item_type = 'track'` rows in `remote_synced_items` so album summary rows are never double-counted.

## Credential Handling

- Target configuration JSON is stored in SQLite (`remote_targets.config_json`).
- Before returning `RemoteTarget` records to the WebView, `crates/app-core/src/remote_secrets.rs` strips `password`, `clientSecret`, `accessToken`, and `refreshToken` and sets `hasPassword` / `hasTokens` indicators.
- When saving edits to an existing target with empty secret fields, previously stored secrets in SQLite are preserved automatically.
