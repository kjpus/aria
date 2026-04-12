# AGENTS.md

## Purpose

This file is project-specific customization for GPT/Codex and other AI agents working inside the Aria repository.

Aria is a desktop-first classical music player with:

- a Rust service core
- a Tauri shell
- a React + TypeScript frontend
- a local-library workflow where tags, playback quality, and classical metadata are first-class concerns

The design target is a polished desktop product, not a generic web app in a web view.

## Product Snapshot

Aria currently has these top-level panes:

- `Library`: album browser with cover art
- `Album`: selected-album detail view and album-level actions
- `Tracks`: grouped track browser with configurable columns and sorting
- `Playlists`: persistent playlists with collage artwork and playback actions
- `Queue`: persistent playback queue
- `Settings`: library directories, database field mappings, catalog rules, and playback preferences

Current product capabilities include:

- local-library scanning into SQLite
- persistent raw tags and normalized mapped fields
- configurable database field mappings
- configurable composer-aware catalog parsing rules
- playlist persistence
- queue persistence
- Windows playback with shared and exclusive WASAPI paths

## Architecture Guardrails

- Keep the frontend presentation-focused.
- Keep `src-tauri` thin.
- Put business logic in Rust crates under `/crates`.
- Treat playback as a long-lived service, not a set of ad hoc command handlers.
- Do not let real-time audio paths depend on SQLite, artwork decoding, or heavy locks.
- Model classical metadata explicitly. Do not collapse everything into album/artist/track.

## Repository Shape

Current top-level ownership is:

- `/src`: React UI and pane-specific presentation logic
- `/src-tauri`: Tauri command registration and shell integration
- `/crates/domain`: shared DTOs and defaults
- `/crates/app-core`: orchestration across services
- `/crates/library`: scan pipeline, raw tags, mapping, catalog extraction, artwork lookup
- `/crates/playback`: playback service, queue, Windows output backends
- `/crates/playlists`: playlist service and mutations
- `/crates/storage`: SQLite persistence and migrations
- `/crates/artwork`, `/crates/search`, `/crates/platform-audio`: reserved or partial support crates

If you add a new directory, keep ownership boundaries obvious.

## Default Technical Choices

Unless there is a strong reason to change course:

- Frontend: React + TypeScript + Vite
- Backend: Rust + Tokio
- Persistence: SQLite
- IPC style: Tauri commands for request/response, app events for streamed state

## Metadata Rules

- Preserve raw file tags separately from normalized Aria fields.
- Support multi-value credits and roles.
- Keep composer, work, movement, performer, ensemble, and conductor as first-class concepts.
- Avoid encoding classical structure only in display strings.
- Do not silently discard uncommon raw tags just because they are not part of the default field list.

## Library Rules

- The library root list is part of persistent state.
- Adding a new library directory should be safe to repeat; duplicate roots should be rejected cleanly.
- Field mappings are user-configurable and persisted.
- Catalog extraction rules are user-configurable and persisted.
- If scan behavior changes materially, update `docs/LIBRARY.md`.

## Playback Rules

- Optimize first for correctness and predictable state flow.
- Desktop playback quality matters more than mobile reach in early milestones.
- PCM-first is acceptable for v1.
- Treat DSD, DoP, and advanced DAC features as staged enhancements unless explicitly in scope.
- Do not mislabel shared-mode output as bit-perfect.

## UX Guardrails

- Preserve the desktop-native feel.
- Avoid generic dashboard UI patterns when a music-product interaction is more appropriate.
- Keep the top-level layout dense and intentional; do not reintroduce large ornamental padding without a product reason.
- Context menus, tables, queue behavior, and playlist behavior should stay internally consistent across panes.

## Working Style For Agents

- Inspect the existing code before changing behavior.
- Prefer small vertical slices that touch one service boundary at a time.
- Prefer typed DTOs between crates and between backend and frontend.
- Add or update targeted tests when fixing tag parsing or scan edge cases.
- Keep logs useful, but avoid noisy logs in playback loops.
- Document architecture changes in `aria_plan.md` when they materially affect crate ownership or the app model.
- Update `README.md` when developer or user workflows change.

## Verification Defaults

For normal UI or integration changes, prefer these checks:

- `npm.cmd run build`
- `cargo check --manifest-path src-tauri/Cargo.toml`

If the change touches scan logic, tag parsing, or catalog extraction, also look for an appropriate targeted Rust test in `crates/library`.

## Things To Avoid

- frontend code reading the filesystem directly
- Tauri commands opening their own unmanaged database connections for one-off work
- UI state becoming the source of truth for playback position or transport
- audio-thread code waiting on async tasks unrelated to output timing
- locking the product into only album-based browsing
