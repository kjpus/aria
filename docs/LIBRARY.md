# Library And Metadata Pipeline

This document describes how Aria scans a local music library, stores metadata, and turns raw file tags into the fields used by the UI.

## Goals

Aria treats the library as two layers:

- `raw tags`: what is read directly from the media file
- `mapped fields`: the normalized database-facing fields that the UI uses for browsing, sorting, grouping, and playback context

That split is intentional. Raw tags must remain available even when Aria’s normalized view changes.

## Scan Overview

When Aria scans the library, it does this for each configured root:

1. Recursively discover supported audio files
2. Read tag data and audio properties from each file
3. Preserve raw tags
4. Apply user-configurable field mappings
5. Apply catalog fallback rules for the `catalog` field when needed
6. Resolve album art
7. Persist the resulting snapshot to SQLite

The scan result is stored in the `LibrarySnapshot`, which includes:

- library roots
- scan status and file counts
- field mappings
- catalog rules
- tag inventory
- scanned tracks

## Supported Audio File Discovery

The scanner currently includes files with these extensions:

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

Discovery is recursive and follows links.

## Track Model

Each scanned track stores:

- `id`: currently the full file path
- `path`
- `file_name`
- `album_art_path`
- `audio`: format, duration, sample rate, bit depth, channels
- `raw_tags`: `tag -> [values]`
- `mapped_fields`: `field -> [values]`

## Raw Tag Extraction

Aria reads tags with Lofty, then supplements that with format-specific raw-tag recovery where needed.

### Base extraction

The base pass:

- iterates all Lofty tags on the file
- resolves each tag key to a normalized uppercase tag name
- converts tag values to strings
- splits multi-value text when Aria sees common separators
- deduplicates values while preserving order

### Multi-value splitting

Aria currently splits text values on:

- `;`
- ` / `
- ` | `
- the NUL character

This is intentionally conservative. The goal is to preserve multi-value credits without over-splitting ordinary text.

### Format-specific raw tags

Some formats, especially Vorbis-comment based formats, can contain useful custom keys that do not survive a purely generic tag abstraction.

To preserve those, Aria explicitly merges raw Vorbis-comment data for:

- FLAC
- Ogg Vorbis
- Opus
- Speex

That is why tags like `ENSEMBLE` can still appear in `raw_tags` even if the generic tag path would otherwise lose them.

## Tag Inventory

The scan also builds a tag inventory. For each observed raw tag, Aria stores:

- tag name
- number of tracks where it occurred
- up to three example values

This inventory is useful for field-mapping and diagnostics, even though the main Settings pane no longer shows it inline.

## Database Field Mappings

Field mappings define how Aria builds normalized fields from raw tags.

Each mapping has:

- `key`: internal field name
- `label`: UI label
- `tag_priorities`: a priority-ordered list of raw tags

### Resolution rule

For a given field:

- Aria checks source tags in order
- the first non-empty source tag wins
- all values from that winning tag are kept
- duplicate values are removed while preserving order

Aria does not merge across multiple source tags for a single field. Priority is strict.

### Default fields

The default field list is:

| Field key | Default source tags |
| --- | --- |
| `album` | `ALBUM` |
| `title` | `TITLE` |
| `catalog` | `CATALOGNUMBER`, `CATALOG` |
| `composer` | `COMPOSER` |
| `genre` | `GENRE` |
| `conductor` | `CONDUCTOR` |
| `ensemble` | `ENSEMBLE`, `ORCHESTRA`, `ALBUMARTIST` |
| `soloist` | `PERFORMER`, `ARTIST`, `ALBUMARTIST` |
| `year` | `DATE`, `YEAR` |
| `disk_number` | `DISCNUMBER` |
| `track_number` | `TRACKNUMBER` |

Users can edit these in `Settings -> Database fields`.

Fields may be empty. They may also contain multiple values.

## Catalog Extraction

The `catalog` field is special.

### Dedicated tags first

Aria first tries to resolve `catalog` from the configured field mapping, which defaults to:

- `CATALOGNUMBER`
- `CATALOG`

If that succeeds, catalog fallback parsing is not used.

### Fallback rules

If `catalog` is still empty after normal field mapping, Aria runs user-configurable catalog rules.

Each catalog rule has:

- `label`
- `pattern`: regex
- `composers`: optional composer hints
- `source_tags`: ordered source-tag list
- `enabled`

Users can edit these in `Settings -> Catalog rules`.

### Current built-in examples

Aria ships with built-in rules for common classical catalogs, including examples such as:

- `Opus`
- `BWV`
- `WAB`
- `K`
- `KV`
- `D`
- `RV`
- `HWV`
- `TWV`
- `BuxWV`
- `Hob.`

### Source-tag priority

Catalog rules respect source-tag priority.

If a rule is configured with:

- `TITLE`
- `WORK`
- `ALBUM`

then Aria:

- tries `TITLE` first
- only falls back to `WORK` if `TITLE` produced no matches
- only falls back to `ALBUM` if neither `TITLE` nor `WORK` produced matches

This avoids leaking album-level range catalogs into track-level results when the track title already contains the specific catalog number.

### Composer-aware matching

If a rule has composer hints, Aria only applies it when one of these raw tags matches those hints:

- `COMPOSER`
- `WORKCOMPOSER`
- `COMPOSERSORT`

This is how rules like `WAB` remain Bruckner-specific.

### Colon-segment preference

When a title contains multiple colon-separated segments, Aria searches the segments from right to left and keeps the first segment that yields catalog matches.

Example:

```text
Das Wohltemperierte Klavier: Book 1, BWV 846-869: Präludium Es-Dur, BWV 852
```

Aria prefers `BWV 852` from the final segment instead of the collection-level range earlier in the string.

### Range suppression

Aria ignores regex matches that are immediately followed by a dash and more digits, such as:

```text
BWV 846-869
```

That prevents the start of a catalog range from being treated as a single-track catalog number.

## Album Art Resolution

Album art is resolved in this order:

1. Embedded FLAC cover art
2. Sidecar image files

### Embedded FLAC art

For FLAC files, Aria:

- reads embedded pictures
- prefers `CoverFront`, then other picture types in a stable priority order
- writes the extracted image into a local app-data cache
- reuses the cached image on later scans if it already exists and is non-empty

### Sidecar fallback

If no embedded FLAC art is available, Aria looks for sidecar files named:

- `cover.jpg`
- `folder.jpg`
- `front.jpg`
- `cover.png`
- `folder.png`
- `front.png`

It searches:

- the track directory
- the parent directory as a fallback when the track is inside a disc-like folder such as `Disc 1` or `CD1`

Zero-byte sidecar files are ignored.

### Current limitation

Embedded art extraction is currently FLAC-first. Other formats mainly rely on sidecar images for now.

## Persistence In SQLite

Aria persists the library state into SQLite tables that include:

- `library_state`
- `library_roots`
- `field_mappings`
- `catalog_rules`
- `tag_inventory`
- `scanned_tracks`

Important details:

- `raw_tags` are stored as JSON
- `mapped_fields` are stored as JSON
- audio properties are stored as JSON
- library settings and playback state are also persisted elsewhere in the same database

On Windows, the default database path is:

```text
%LOCALAPPDATA%\Aria\aria.sqlite3
```

## Settings And Re-scan Behavior

- Adding a new library directory from Settings starts a scan automatically
- Re-saving field mappings remaps existing scanned tracks from stored raw tags
- Re-saving catalog rules also remaps existing scanned tracks from stored raw tags
- A full rescan is still needed when the source files themselves changed or when album-art lookup behavior needs to be refreshed

## Raw Tags In The UI

The `Tracks` tab has a `Show all tags` action. That dialog reads raw tags directly from the selected file on demand, rather than showing only the normalized database fields.

This is useful when debugging:

- missing fields
- unexpected catalog results
- multi-value role mapping
- rare custom tags

## Practical Debugging Rules

If a field looks wrong, inspect in this order:

1. raw file tags
2. current field mapping
3. current catalog rules, if the field is `catalog`
4. whether the track needs a rescan

If scan behavior changes, update this document so it stays aligned with the code.
