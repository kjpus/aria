# Library And Metadata Pipeline

This document describes how Aria scans local music libraries, merges remote library metadata, normalizes raw tags into per-format database fields, resolves artwork, and writes tag edits back to audio files.

## Goals

Aria models track metadata in two layers:

- **`raw_tags`**: Tag key/value pairs read directly from the media file.
- **`mapped_fields`**: Normalized database-facing fields used by the UI for browsing, sorting, grouping, search, and playback context.

Raw tags are preserved alongside mapped fields so users can adjust field priorities or catalog rules and remap their library without losing uncommon or custom file tags.

## Scan Overview

When scanning local roots, Aria runs the following pipeline:

1. Recursively discover supported audio files across configured roots, skipping hidden/dot directories and Windows system folders (`$RECYCLE.BIN`, `System Volume Information`), and deduplicating overlapping roots.
2. Read audio properties and tags from each file via `lofty`, then merge format-specific raw tags (Vorbis comments for FLAC/OGG/OPUS/Speex and freeform `ilst` atoms for MP4/M4A).
3. Apply per-format user-configurable field mappings (`DEFAULT` + format-specific profile).
4. Apply composer-aware catalog fallback rules for `catalog` when dedicated catalog tags are empty.
5. Resolve album art (embedded FLAC or MP4/M4A artwork first, then sidecar images).
6. Merge any existing remote (`remote://`) tracks so local scans do not discard synced remote libraries.
7. Persist the updated `LibrarySnapshot` to SQLite.

`LibrarySnapshot` stores:

- `roots`: configured local `LibraryRoot` entries
- `is_scanning`, `indexed_files`, `last_scan_at`
- `field_mappings`: per-format `LibraryFieldMapping` rows
- `catalog_rules`: composer-aware `CatalogRule` rows
- `tag_inventory`: observed raw tags with occurrence counts and up to 3 example values
- `tracks`: scanned local tracks and synced `remote://` tracks

## Supported Audio File Discovery

The scanner discovers files with these extensions (case-insensitive):

- `flac`
- `mp3`
- `m4a`
- `aac`
- `mp4`
- `ogg`
- `opus`
- `wav`
- `aiff`
- `aif`

Discovery is recursive and follows symlinks. Path separators are normalized on Windows and canonicalized/deduplicated so overlapping roots (e.g., `D:\Music` and `D:\Music\Bach`) never produce duplicate `scanned_tracks.id` rows.

## Track Model

Each `ScannedTrack` stores:

- `id`: full file path for local tracks, or canonical deterministic ID for remote tracks
- `path`: local filesystem path or `remote://<target_id>/<relative_path>`
- `file_name`
- `album_art_path`: local cached embedded cover art path or sidecar cover path
- `audio`: `format`, `duration_ms`, `sample_rate`, `bit_depth`, `channels`
- `raw_tags`: `BTreeMap<String, Vec<String>>`
- `mapped_fields`: `BTreeMap<String, Vec<String>>` (remote tracks also carry synthetic `album_path`)

## Raw Tag Extraction

Aria reads tags with `lofty` and supplements generic tag iteration with format-specific extractors:

- **Base extraction:** Iterates all tags on the file, maps `ItemKey` to format-native tag names (e.g., `TALB`/`TIT2`/`TCOM` for ID3v2, `©ALB`/`©NAM`/`©WRT` for MP4, `IPRD`/`INAM` for RIFF INFO, `NAME`/`AUTH` for AIFF), splits multi-value strings, and deduplicates values while preserving order.
- **Multi-value splitting:** Splits text on `;`, ` / `, ` | `, and NUL (`\0`).
- **Tag name normalization:** Uppercases standard tag keys while preserving the canonical `com.apple.iTunes` namespace casing for MP4 freeform atoms (`----:com.apple.iTunes:<NAME>`).
- **Format-specific raw-tag recovery:**
  - **FLAC, Ogg Vorbis, Opus:** Merges raw `VorbisComments` so custom classical tags like `ENSEMBLE` and `WORKCOMPOSER` are always preserved.
  - **MP4 / M4A:** Merges raw `Ilst` freeform atoms (`----:<mean>:<name>`) so custom iTunes freeform classical tags (`----:com.apple.iTunes:ENSEMBLE`, `CONDUCTOR`, `SOLOIST`, `CATALOGNUMBER`, etc.) are preserved even when `lofty`'s generic `Tag` view omits them.

## Per-Format Database Field Mappings

Field mappings are defined per canonical format key in [`crates/domain/src/library.rs`](../crates/domain/src/library.rs):

- `DEFAULT` (fallback profile)
- `FLAC`
- `MP3`
- `MP4` (also used for `.m4a`)
- `AAC`
- `OGG`
- `OPUS`
- `WAV`
- `AIFF` (also used for `.aif`)

Each `LibraryFieldMapping` has:

- `format`: canonical format key
- `key`: internal database field name
- `label`: UI label
- `tag_priorities`: priority-ordered list of raw tag names

### Resolution rule

1. Aria applies `DEFAULT` mappings first, then overrides with any mappings matching the track's canonical format.
2. For each mapping, Aria checks `tag_priorities` in order (matching raw tags case-insensitively).
3. The **first non-empty source tag wins**; all values from that tag are kept and deduplicated in order.
4. When users save updated mappings or catalog rules, Aria remaps existing tracks from stored `raw_tags` while preserving synthetic remote `album_path` entries and keeping remote tracks that have no local `raw_tags`.

### Default field profiles (summary)

| Field key | `DEFAULT` / `FLAC` / `OGG` / `OPUS` | `MP3` / `AAC` (ID3v2) | `MP4` / `M4A` | `WAV` / `AIFF` |
| --- | --- | --- | --- | --- |
| `album` | `ALBUM` | `TALB`, `ALBUM` | `©ALB`, `ALBUM` | `TALB`, `IPRD` (WAV), `ALBUM` |
| `title` | `TITLE` | `TIT2`, `TITLE` | `©NAM`, `TITLE` | `TIT2`, `INAM` (WAV) / `NAME` (AIFF), `TITLE` |
| `catalog` | `CATALOGNUMBER`, `CATALOG` | `CATALOGNUMBER`, `CATALOG` | `----:com.apple.iTunes:CATALOGNUMBER`, `----:com.apple.iTunes:CATALOG`, `CATALOGNUMBER`, `CATALOG` | `CATALOGNUMBER`, `CATALOG` |
| `composer` | `COMPOSER`, `WORKCOMPOSER`, `COMPOSERSORT` | `TCOM`, `COMPOSER`, `WORKCOMPOSER`, `COMPOSERSORT` | `©WRT`, `COMPOSER`, `----:com.apple.iTunes:COMPOSER`, `WORKCOMPOSER`, `COMPOSERSORT` | `TCOM`, `IMUS` (WAV), `COMPOSER`, `WORKCOMPOSER`, `COMPOSERSORT` |
| `genre` | `GENRE` | `TCON`, `GENRE` | `©GEN`, `GNRE`, `GENRE` | `TCON`, `IGNR` (WAV), `GENRE` |
| `conductor` | `CONDUCTOR` | `TPE3`, `CONDUCTOR` | `----:com.apple.iTunes:CONDUCTOR`, `CONDUCTOR` | `TPE3`, `CONDUCTOR` |
| `ensemble` | `ENSEMBLE`, `ORCHESTRA`, `BAND`, `CHOIR`, `CHORUS` | `BAND`, `ORCHESTRA`, `ENSEMBLE`, `CHOIR`, `CHORUS` | `----:com.apple.iTunes:ENSEMBLE`, `ORCHESTRA`, `BAND`, `CHOIR`, plus plain fallbacks | `BAND`, `ORCHESTRA`, `ENSEMBLE`, `CHOIR`, `CHORUS` |
| `soloist` | `SOLOIST`, `PERFORMER`, `ARTIST`, `ALBUMARTIST` | `SOLOIST`, `TPE1`, `ARTIST`, `PERFORMER`, `TPE2`, `ALBUMARTIST` | `----:com.apple.iTunes:SOLOIST`, `PERFORMER`, `SOLOIST`, `PERFORMER`, `©ART`, `ARTIST`, `AART`, `ALBUMARTIST` | `SOLOIST`, `TPE1`, `IART` (WAV) / `AUTH` (AIFF), `ARTIST`, `PERFORMER`, `TPE2`, `ALBUMARTIST` |
| `year` | `DATE`, `YEAR` | `TDRC`, `TYER`, `TDOR`, `DATE`, `YEAR` | `©DAY`, `DATE`, `YEAR` | `TDRC`, `TYER`, `ICRD` (WAV), `DATE`, `YEAR` |
| `disk_number` | `DISCNUMBER`, `DISKNUMBER`, `DISC` | `TPOS`, `DISCNUMBER`, `DISKNUMBER`, `DISC` | `DISK`, `DISCNUMBER`, `DISKNUMBER`, `DISC` | `TPOS`, `DISCNUMBER`, `DISKNUMBER`, `DISC` |
| `track_number` | `TRACKNUMBER`, `TRACK` | `TRCK`, `TRACKNUMBER`, `TRACK` | `TRKN`, `TRACKNUMBER`, `TRACK` | `TRCK`, `IPRT`/`ITRK` (WAV), `TRACKNUMBER`, `TRACK` |

Users can edit all per-format profiles in `Settings -> Database fields`.

## Catalog Extraction

If the `catalog` field is resolved from dedicated catalog tags (`CATALOGNUMBER`, `CATALOG`, etc.), fallback parsing is skipped. Otherwise, Aria runs user-configurable `CatalogRule` definitions (`label`, optional `composers` hints, `enabled`).

### Built-in catalog labels

`BWV`, `WAB`, `K`, `KV`, `D`, `RV`, `HWV`, `TWV`, `BuxWV`, `Hob.`, `S.`, `WoO`, `Op` (catch-all fallback).

### Format-aware source tiers

Aria searches three priority tiers in order, stopping at the first tier that produces matches:

1. **Title tags:** `TITLE`, `©NAM`, `TIT2`, `INAM`, `NAME`
2. **Work tags:** `WORK`, `©WRK`, `TIT1`, `----:com.apple.iTunes:WORK`
3. **Album tags:** `ALBUM`, `©ALB`, `TALB`, `IPRD`

Within each string, Aria splits colon-separated segments and searches **right to left**, preferring the most specific movement/work segment (e.g., `BWV 852` over a book-level range `BWV 846-869`). Range prefixes followed by `-<digits>` are suppressed, and sectioned Roman-numeral catalogs such as `Hob. XVI:52` and `Hob. IIIb:2` are supported.

### Composer-aware matching

Rules with `composers` hints check composer tags across formats (`COMPOSER`, `©WRT`, `TCOM`, `IMUS`, `----:com.apple.iTunes:COMPOSER`, `WORKCOMPOSER`, `COMPOSERSORT`) using Unicode diacritic folding via `deunicode` (so `"Georg Friedrich Händel"` matches `"handel"`):

- When composer tags are present, a composer-hinted rule only applies if one of its composer hints matches.
- When composer tags are missing, **unambiguous multi-letter labels** (such as `HWV`, `BWV`, `BuxWV`) may still match, while **ambiguous single-letter labels** (`K`, `D`, `S.`) are blocked to prevent false positives.

## Album Art Resolution

Album art is resolved in this order:

1. **Embedded FLAC cover art** (`CoverFront` preferred, cached to local app-data cache).
2. **Embedded MP4 / M4A cover art** (`covr` atom, cached to local app-data cache).
3. **Sidecar images** (`cover.jpg`, `folder.jpg`, `front.jpg`, `cover.png`, `folder.png`, `front.png`) in the track directory, falling back to the parent directory when the track resides in a disc folder (`Disc 1`, `CD1`, etc.). Zero-byte sidecar files are ignored.

## In-Place Tag Editing And Field Export

Aria supports editing raw tags directly in local audio files (`edit_track_tags`) and exporting normalized mapped fields to target raw tags (`export_mapped_field_to_tag`):

- **Supported writable containers:** FLAC/Vorbis/Opus (`VorbisComments`), MP3/WAV/AIFF (`Id3v2Tag`), and MP4/M4A (`Ilst`).
- **Concurrency & safety:** Tag writes acquire a service-level `write_lock`, batch all field updates per track into a single file save, close open read handles before rewriting on Windows, remove legacy uppercased duplicate MP4 freeform atoms (`----:COM.APPLE.ITUNES:*`), and refresh already-succeeded tracks even if a later file in a batch fails.

## Persistence In SQLite

Library and remote sync state are stored in SQLite tables:

- `library_state`, `library_roots`, `field_mappings`, `catalog_rules`, `tag_inventory`, `scanned_tracks`
- `remote_targets`, `remote_synced_items` (see [REMOTE_STORAGE.md](./REMOTE_STORAGE.md))

SQLite connections enable `PRAGMA foreign_keys = ON` and a 5-second `busy_timeout`.

## Debugging Metadata Issues

Use **Show all tags** (available in `Tracks` and `Player`) to read raw tags directly from disk. When a field looks wrong, check in order:

1. Raw file tags in the inspector
2. Per-format mapping priority in `Settings -> Database fields`
3. Catalog rules in `Settings -> Catalog rules` (if `catalog` is unexpected)
4. Trigger a rescan if the file changed outside Aria
