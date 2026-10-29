use aria_domain::{
    AppBootstrap, AppEvent, CatalogRule, FieldExportRequest, LibraryEvent, LibraryFieldMapping,
    LibrarySnapshot, OutputDeviceSnapshot, PlayTrackRequest, PlaybackEvent, PlaybackPreferences,
    PlaybackSessionSnapshot, PlaybackSnapshot, PlaybackStatus, Playlist, PlaylistEvent,
    PlaylistImportPreview, PlaylistSnapshot, PreviewTrack, RemoteBackendType, RemoteEvent,
    RemotePlaylistTrack, RemoteSyncCompletedEvent, RemoteSyncProgressEvent, RemoteSyncedItem,
    RemoteTarget, ScannedTrack, SettingsSnapshot, ThemePreference, TrackTableSettings,
    TrackTagEditRequest, UploadCompletedEvent, UploadFailedItem, UploadProgressEvent,
};
use aria_library::manifest::{build_album_manifest, derive_systematic_folder_name};
use aria_library::{LibraryError, LibraryService};
use aria_playback::{PlaybackError, PlaybackService};
use aria_playlists::{PlaylistError, PlaylistService};
use aria_remote_storage::{
    create_backend_for_target, sync_remote_library, sync_remote_playlists, upload_album,
    upload_playlist, GoogleAuthConfig, GoogleDriveStoredConfig, PathUpdater, PendingAuthFlow,
    PlaybackPrefetcher, PrefetchCandidate, RemoteCacheManager, RemoteCacheStatus,
    RemoteStorageError, StorageStatus, UploadOptions,
};
use aria_storage::{AppDatabase, SettingsStore, StorageError};
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{broadcast, Mutex};

#[derive(Debug, Error)]
pub enum AppCoreError {
    #[error(transparent)]
    Library(#[from] LibraryError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Playback(#[from] PlaybackError),
    #[error(transparent)]
    Playlist(#[from] PlaylistError),
    #[error(transparent)]
    RemoteStorage(#[from] RemoteStorageError),
    #[error("failed to import playlist: {0}")]
    Import(String),
    #[error("remote error: {0}")]
    Remote(String),
}

static GLOBAL_PENDING_GDRIVE_AUTH: Mutex<Option<PendingAuthFlow>> = Mutex::const_new(None);
static GLOBAL_LAST_COMPLETED_GDRIVE_TARGET: Mutex<Option<RemoteTarget>> = Mutex::const_new(None);

#[derive(Clone)]
pub struct AppCore {
    events: broadcast::Sender<AppEvent>,
    library: LibraryService,
    playback: PlaybackService,
    playlists: PlaylistService,
    settings: SettingsStore,
    database: AppDatabase,
    cache_manager: Arc<RemoteCacheManager>,
    prefetcher: Arc<PlaybackPrefetcher>,
}

impl AppCore {
    pub fn new() -> Result<Self, AppCoreError> {
        let database = AppDatabase::new_default()?;
        let persisted = database.load_state()?;
        let (events, _) = broadcast::channel(512);
        let playback = PlaybackService::with_session(
            persisted.settings.playback.clone(),
            persisted.playback,
        );

        let cache_dir = RemoteCacheManager::default_cache_dir().unwrap_or_else(|_| {
            std::env::temp_dir().join("aria_remote_cache")
        });
        let cache_manager = Arc::new(RemoteCacheManager::new(cache_dir, 8));

        let playback_for_updater = playback.clone();
        let path_updater: PathUpdater = Arc::new(move |track_id, path| {
            playback_for_updater.update_queue_track_path(track_id, path.to_string());
        });

        let prefetcher = Arc::new(PlaybackPrefetcher::spawn(
            cache_manager.clone(),
            Some(events.clone()),
            Some(path_updater),
        ));

        let core = Self {
            events,
            library: LibraryService::with_snapshot(persisted.library),
            playback,
            playlists: PlaylistService::with_snapshot(persisted.playlists),
            settings: SettingsStore::with_snapshot(persisted.settings),
            database: database.clone(),
            cache_manager,
            prefetcher,
        };

        core.spawn_persistence_worker(database);
        core.spawn_playback_publisher();
        core.spawn_output_device_monitor();
        core.spawn_remote_sync_worker();
        Ok(core)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.events.subscribe()
    }

    pub async fn bootstrap(&self) -> AppBootstrap {
        AppBootstrap {
            library: self.library.snapshot().await,
            playback: self.playback.snapshot().await,
            playlists: self.playlists.snapshot().await,
            settings: self.settings.snapshot().await,
            remote_targets: self.database.load_remote_targets().unwrap_or_default(),
        }
    }

    pub fn shutdown(&self) {
        self.playback.shutdown();
    }

    pub async fn add_library_root(&self, path: String) -> Result<LibrarySnapshot, AppCoreError> {
        let snapshot = self.library.add_root(path).await?;
        self.emit(AppEvent::Library(LibraryEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn remove_library_root(&self, path: String) -> Result<LibrarySnapshot, AppCoreError> {
        let snapshot = self.library.remove_root(path).await;
        self.emit(AppEvent::Library(LibraryEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn clear_library(&self) -> Result<LibrarySnapshot, AppCoreError> {
        let snapshot = self.library.clear().await;
        self.emit(AppEvent::Library(LibraryEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        let playlist_snapshot = self.playlists.clear().await;
        self.emit(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(
            playlist_snapshot,
        )));
        Ok(snapshot)
    }

    pub async fn create_playlist(
        &self,
        name: String,
        track_ids: Vec<String>,
    ) -> Result<PlaylistSnapshot, AppCoreError> {
        let snapshot = self.playlists.create_playlist(name, track_ids).await?;
        self.emit(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn add_tracks_to_playlist(
        &self,
        playlist_id: String,
        track_ids: Vec<String>,
    ) -> Result<PlaylistSnapshot, AppCoreError> {
        let snapshot = self.playlists.add_tracks(playlist_id, track_ids).await?;
        self.emit(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn rename_playlist(
        &self,
        playlist_id: String,
        name: String,
    ) -> Result<PlaylistSnapshot, AppCoreError> {
        let snapshot = self.playlists.rename_playlist(playlist_id, name).await?;
        self.emit(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn delete_playlist(
        &self,
        playlist_id: String,
    ) -> Result<PlaylistSnapshot, AppCoreError> {
        let snapshot = self.playlists.delete_playlist(playlist_id).await?;
        self.emit(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn regenerate_playlist_icon(
        &self,
        playlist_id: String,
    ) -> Result<PlaylistSnapshot, AppCoreError> {
        let snapshot = self.playlists.regenerate_icon(playlist_id).await?;
        self.emit(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn build_playlist_m3u(
        &self,
        playlist_id: String,
    ) -> Result<(String, String), AppCoreError> {
        let playlists = self.playlists.snapshot().await;
        let playlist = playlists
            .playlists
            .iter()
            .find(|playlist| playlist.id == playlist_id)
            .cloned()
            .ok_or(PlaylistError::PlaylistNotFound)?;
        let library = self.library.snapshot().await;
        let track_lookup = library
            .tracks
            .into_iter()
            .map(|track| (track.id.clone(), track))
            .collect::<BTreeMap<_, _>>();

        let mut lines = vec!["#EXTM3U".to_string()];
        for track_id in &playlist.track_ids {
            let Some(track) = track_lookup.get(track_id) else {
                continue;
            };
            let display_title = track
                .mapped_fields
                .get("title")
                .and_then(|values| values.first())
                .cloned()
                .unwrap_or_else(|| track.file_name.clone());
            let duration_secs = track.audio.duration_ms / 1000;
            lines.push(format!("#EXTINF:{duration_secs},{display_title}"));
            lines.push(track.path.clone());
        }

        Ok((
            format!("{}.m3u", sanitize_playlist_filename(&playlist.name)),
            lines.join("\r\n"),
        ))
    }

    pub fn get_system_default_codepage(&self) -> u32 {
        get_system_default_codepage()
    }

    pub async fn get_playlist_import_preview(
        &self,
        file_path: String,
        codepage: Option<u32>,
    ) -> Result<PlaylistImportPreview, AppCoreError> {
        let path_obj = std::path::Path::new(&file_path);
        let extension = path_obj
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .unwrap_or_default();

        let bytes = std::fs::read(&file_path)
            .map_err(|err| AppCoreError::Import(format!("failed to read playlist file: {err}")))?;

        let system_cp = get_system_default_codepage();
        let selected_cp = codepage.unwrap_or(system_cp);

        let content = decode_with_codepage(&bytes, selected_cp)
            .map_err(|err| AppCoreError::Import(format!("failed to decode playlist: {err}")))?;

        let raw_tracks = parse_playlist_content(&content, &extension);
        let parent_dir = path_obj.parent().unwrap_or_else(|| std::path::Path::new(""));

        let library = self.library.snapshot().await;
        let matcher = PlaylistTrackMatcher::new(&library.tracks);

        let mut preview_tracks = Vec::new();
        for (raw_path, raw_title) in raw_tracks {
            let p = std::path::Path::new(&raw_path);
            let resolved = if p.is_absolute() {
                p.to_path_buf()
            } else {
                parent_dir.join(p)
            };
            let track_id = matcher.find_match(&resolved);

            let title = raw_title.unwrap_or_else(|| {
                p.file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(&raw_path)
                    .to_string()
            });

            preview_tracks.push(PreviewTrack {
                title,
                path: resolved.to_string_lossy().into_owned(),
                track_id,
            });
        }

        let name = path_obj
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Imported Playlist")
            .to_string();

        Ok(PlaylistImportPreview {
            file_path,
            name,
            codepage: selected_cp,
            system_default_codepage: system_cp,
            tracks: preview_tracks,
        })
    }

    pub async fn commit_playlist_import(
        &self,
        file_path: String,
        name: String,
        codepage: u32,
    ) -> Result<PlaylistSnapshot, AppCoreError> {
        let preview = self.get_playlist_import_preview(file_path, Some(codepage)).await?;
        let matched_track_ids: Vec<String> = preview
            .tracks
            .into_iter()
            .filter_map(|t| t.track_id)
            .collect();

        if matched_track_ids.is_empty() {
            return Err(AppCoreError::Import(
                "No matching tracks from the playlist were found in your library.".into(),
            ));
        }

        self.create_playlist(name, matched_track_ids).await
    }

    pub async fn remove_tracks_from_playlist(
        &self,
        playlist_id: String,
        track_ids: Vec<String>,
    ) -> Result<PlaylistSnapshot, AppCoreError> {
        let snapshot = self.playlists.remove_tracks(playlist_id, track_ids).await?;
        self.emit(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn reorder_playlist_tracks(
        &self,
        playlist_id: String,
        track_ids: Vec<String>,
    ) -> Result<PlaylistSnapshot, AppCoreError> {
        let snapshot = self.playlists.reorder_tracks(playlist_id, track_ids).await?;
        self.emit(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn reorder_playlists(
        &self,
        playlist_ids: Vec<String>,
    ) -> Result<PlaylistSnapshot, AppCoreError> {
        let snapshot = self.playlists.reorder_playlists(playlist_ids).await?;
        self.emit(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn set_field_mappings(&self, mappings: Vec<LibraryFieldMapping>) -> LibrarySnapshot {
        let snapshot = self.library.set_field_mappings(mappings).await;
        self.emit(AppEvent::Library(LibraryEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        snapshot
    }

    pub async fn set_catalog_rules(
        &self,
        rules: Vec<CatalogRule>,
    ) -> Result<LibrarySnapshot, AppCoreError> {
        let snapshot = self.library.set_catalog_rules(rules).await?;
        self.emit(AppEvent::Library(LibraryEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn start_library_scan(&self) -> Result<(), AppCoreError> {
        self.library.start_scan(self.events.clone()).await?;
        Ok(())
    }

    pub async fn read_track_raw_tags(
        &self,
        path: String,
    ) -> Result<BTreeMap<String, Vec<String>>, AppCoreError> {
        Ok(self.library.read_track_raw_tags(path).await?)
    }

    pub async fn export_field_to_tag(
        &self,
        request: FieldExportRequest,
    ) -> Result<LibrarySnapshot, AppCoreError> {
        let snapshot = self.library.export_field_to_tag(request).await?;
        self.emit(AppEvent::Library(LibraryEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn edit_track_tags(
        &self,
        request: TrackTagEditRequest,
    ) -> Result<LibrarySnapshot, AppCoreError> {
        let snapshot = self.library.edit_track_tags(request).await?;
        self.emit(AppEvent::Library(LibraryEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn play(&self) -> Result<PlaybackSnapshot, AppCoreError> {
        let session = self.playback.persisted_session();
        let target_idx = session.current_queue_index.or_else(|| {
            if !session.queue.is_empty() {
                Some(0)
            } else {
                None
            }
        });
        if let Some(idx) = target_idx {
            if let Some(mut req) = session.queue.get(idx).cloned() {
                let window: Vec<String> = session
                    .queue
                    .iter()
                    .skip(idx)
                    .take(3)
                    .map(|r| r.queue_item.id.clone())
                    .collect();
                self.resolve_remote_track_if_needed(&mut req, &window).await?;
            }
        }
        let snapshot = self.playback.play().await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        self.trigger_sliding_window_prefetch().await;
        Ok(snapshot)
    }

    pub async fn play_track(
        &self,
        mut request: PlayTrackRequest,
    ) -> Result<PlaybackSnapshot, AppCoreError> {
        let active_window = vec![request.queue_item.id.clone()];
        self.resolve_remote_track_if_needed(&mut request, &active_window)
            .await?;
        let snapshot = self.playback.play_track(request).await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        self.trigger_sliding_window_prefetch().await;
        Ok(snapshot)
    }

    pub async fn add_to_queue(&self, requests: Vec<PlayTrackRequest>) -> PlaybackSnapshot {
        let snapshot = self.playback.add_to_queue(requests).await;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        self.trigger_sliding_window_prefetch().await;
        snapshot
    }

    pub async fn replace_queue(
        &self,
        mut requests: Vec<PlayTrackRequest>,
        start_playing: bool,
    ) -> Result<PlaybackSnapshot, AppCoreError> {
        if start_playing && !requests.is_empty() {
            let active_window: Vec<String> = requests
                .iter()
                .take(3)
                .map(|r| r.queue_item.id.clone())
                .collect();
            self.resolve_remote_track_if_needed(&mut requests[0], &active_window)
                .await?;
        }
        let snapshot = self.playback.replace_queue(requests, start_playing).await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        if start_playing {
            self.trigger_sliding_window_prefetch().await;
        }
        Ok(snapshot)
    }

    pub async fn previous_track(&self) -> Result<PlaybackSnapshot, AppCoreError> {
        let session = self.playback.persisted_session();
        if let Some(idx) = session.current_queue_index {
            if idx > 0 {
                if let Some(mut prev_req) = session.queue.get(idx - 1).cloned() {
                    let window: Vec<String> = session
                        .queue
                        .iter()
                        .skip(idx - 1)
                        .take(3)
                        .map(|r| r.queue_item.id.clone())
                        .collect();
                    self.resolve_remote_track_if_needed(&mut prev_req, &window)
                        .await?;
                }
            }
        }
        let snapshot = self.playback.previous_track().await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        self.trigger_sliding_window_prefetch().await;
        Ok(snapshot)
    }

    pub async fn next_track(&self) -> Result<PlaybackSnapshot, AppCoreError> {
        let session = self.playback.persisted_session();
        if let Some(idx) = session.current_queue_index {
            if let Some(mut next_req) = session.queue.get(idx + 1).cloned() {
                let window: Vec<String> = session
                    .queue
                    .iter()
                    .skip(idx + 1)
                    .take(3)
                    .map(|r| r.queue_item.id.clone())
                    .collect();
                self.resolve_remote_track_if_needed(&mut next_req, &window)
                    .await?;
            }
        }
        let snapshot = self.playback.next_track().await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        self.trigger_sliding_window_prefetch().await;
        Ok(snapshot)
    }

    pub async fn play_queue_index(&self, index: usize) -> Result<PlaybackSnapshot, AppCoreError> {
        let session = self.playback.persisted_session();
        if let Some(mut req) = session.queue.get(index).cloned() {
            let window: Vec<String> = session
                .queue
                .iter()
                .skip(index)
                .take(3)
                .map(|r| r.queue_item.id.clone())
                .collect();
            self.resolve_remote_track_if_needed(&mut req, &window)
                .await?;
        }
        let snapshot = self.playback.play_queue_index(index).await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        self.trigger_sliding_window_prefetch().await;
        Ok(snapshot)
    }

    async fn resolve_remote_track_if_needed(
        &self,
        request: &mut PlayTrackRequest,
        active_window_ids: &[String],
    ) -> Result<bool, AppCoreError> {
        let track_id = &request.queue_item.id;

        // 1. If already cached locally, update path to local cached path
        if let Some(cached_path) = self.cache_manager.get_cached_path(track_id).await {
            let local_path_str = cached_path.to_string_lossy().to_string();
            request.path = local_path_str.clone();
            self.playback
                .update_queue_track_path(track_id, local_path_str);
            return Ok(true);
        }

        // 2. Identify if this is a remote track
        let (target_id, rel_path, checksum) = if request.path.starts_with("remote://") {
            let stripped = &request.path["remote://".len()..];
            if let Some((t_id, r_path)) = stripped.split_once('/') {
                let chk = self
                    .database
                    .find_remote_synced_item(track_id)
                    .ok()
                    .flatten()
                    .and_then(|s| s.checksum);
                (t_id.to_string(), r_path.to_string(), chk)
            } else {
                return Ok(false);
            }
        } else if !std::path::Path::new(&request.path).exists() {
            if let Ok(Some(synced)) = self.database.find_remote_synced_item(track_id) {
                (synced.remote_target_id, synced.remote_path, synced.checksum)
            } else {
                return Ok(false);
            }
        } else {
            return Ok(false);
        };

        // 3. Emit Buffering snapshot so UI indicates loading
        let mut buffering_snapshot = self.playback.snapshot().await;
        buffering_snapshot.status = PlaybackStatus::Buffering;
        buffering_snapshot.current_track = Some(request.queue_item.clone());
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            buffering_snapshot,
        )));

        // 4. Download and cache track atomically
        let targets = match self.database.load_remote_targets() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[app-core] Failed to load remote targets for track '{track_id}': {e}");
                let mut reset_snapshot = self.playback.snapshot().await;
                reset_snapshot.status = PlaybackStatus::Stopped;
                self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(reset_snapshot)));
                return Err(AppCoreError::Remote(format!("Failed to load remote targets: {e}")));
            }
        };

        let target = match targets.into_iter().find(|t| t.id == target_id) {
            Some(t) => t,
            None => {
                eprintln!("[app-core] Remote target '{target_id}' not found for track '{track_id}'");
                let mut reset_snapshot = self.playback.snapshot().await;
                reset_snapshot.status = PlaybackStatus::Stopped;
                self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(reset_snapshot)));
                return Err(AppCoreError::Remote(format!("Target '{target_id}' not found")));
            }
        };

        let backend = match create_backend_for_target(&target) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[app-core] Failed to instantiate backend for target '{target_id}': {e}");
                let mut reset_snapshot = self.playback.snapshot().await;
                reset_snapshot.status = PlaybackStatus::Stopped;
                self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(reset_snapshot)));
                return Err(AppCoreError::Remote(format!("Failed to create backend: {e}")));
            }
        };

        let local_path = match self
            .cache_manager
            .get_or_download_track(
                backend.as_ref(),
                &rel_path,
                track_id,
                checksum.as_deref(),
                active_window_ids,
            )
            .await
        {
            Ok(p) => p,
            Err(err) => {
                eprintln!("[app-core] Failed to download remote track '{rel_path}' (id: {track_id}): {err}");
                let mut reset_snapshot = self.playback.snapshot().await;
                reset_snapshot.status = PlaybackStatus::Stopped;
                self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(reset_snapshot)));
                return Err(AppCoreError::Remote(format!(
                    "Download failed for '{rel_path}': {err}"
                )));
            }
        };

        let local_path_str = local_path.to_string_lossy().to_string();
        request.path = local_path_str.clone();
        self.playback
            .update_queue_track_path(track_id, local_path_str);

        Ok(true)
    }

    async fn trigger_sliding_window_prefetch(&self) {
        let session = self.playback.persisted_session();
        let Some(current_idx) = session.current_queue_index else {
            return;
        };

        let mut active_window_ids = Vec::new();
        for i in 0..5 {
            if let Some(item) = session.queue.get(current_idx + i) {
                active_window_ids.push(item.queue_item.id.clone());
            }
        }

        let targets = match self.database.load_remote_targets() {
            Ok(t) => t,
            Err(_) => return,
        };

        let mut candidates = Vec::new();
        for i in 1..=3 {
            if let Some(req) = session.queue.get(current_idx + i) {
                let track_id = &req.queue_item.id;
                if self.cache_manager.is_track_cached(track_id).await {
                    continue;
                }

                let (target_id, rel_path, checksum) = if req.path.starts_with("remote://") {
                    let stripped = &req.path["remote://".len()..];
                    if let Some((t_id, r_path)) = stripped.split_once('/') {
                        let chk = self
                            .database
                            .find_remote_synced_item(track_id)
                            .ok()
                            .flatten()
                            .and_then(|s| s.checksum);
                        (t_id.to_string(), r_path.to_string(), chk)
                    } else {
                        continue;
                    }
                } else if !std::path::Path::new(&req.path).exists() {
                    if let Ok(Some(synced)) = self.database.find_remote_synced_item(track_id) {
                        (synced.remote_target_id, synced.remote_path, synced.checksum)
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };

                if let Some(target) = targets.iter().find(|t| t.id == target_id) {
                    candidates.push(PrefetchCandidate {
                        track_id: track_id.clone(),
                        title: req.queue_item.title.clone(),
                        remote_path: rel_path,
                        expected_checksum: checksum,
                        target: target.clone(),
                    });
                }
            }
        }

        if !candidates.is_empty() {
            self.prefetcher
                .notify_prefetch(candidates, active_window_ids)
                .await;
        }
    }

    pub async fn shuffle_queue(&self) -> PlaybackSnapshot {
        let snapshot = self.playback.shuffle_queue().await;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        snapshot
    }

    pub async fn restore_queue_order(&self) -> PlaybackSnapshot {
        let snapshot = self.playback.restore_queue_order().await;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        snapshot
    }

    pub async fn pause(&self) -> PlaybackSnapshot {
        let snapshot = self.playback.pause().await;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        snapshot
    }

    pub async fn stop(&self) -> PlaybackSnapshot {
        let snapshot = self.playback.stop().await;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        snapshot
    }

    pub async fn seek(&self, position_ms: u64) -> Result<PlaybackSnapshot, AppCoreError> {
        let snapshot = self.playback.seek(position_ms).await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub fn list_output_devices(&self) -> Result<Vec<OutputDeviceSnapshot>, AppCoreError> {
        Ok(self.playback.list_output_devices()?)
    }

    pub async fn refresh_output_devices(&self) -> Result<Vec<OutputDeviceSnapshot>, AppCoreError> {
        let devices = self.playback.list_output_devices()?;
        self.emit(AppEvent::Playback(PlaybackEvent::OutputDevicesChanged(
            devices.clone(),
        )));

        if let Some(refresh) = self.playback.handle_output_device_change(&devices).await? {
            self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
                refresh.playback_snapshot,
            )));

            if let Some(updated_preferences) = refresh.updated_preferences {
                let settings_snapshot = self.settings.update_playback(updated_preferences).await;
                self.emit(AppEvent::Settings(settings_snapshot));
            }
        }

        Ok(devices)
    }

    pub async fn update_theme(&self, theme: ThemePreference) -> SettingsSnapshot {
        let snapshot = self.settings.update_theme(theme).await;
        self.emit(AppEvent::Settings(snapshot.clone()));
        snapshot
    }

    pub async fn update_track_table_settings(
        &self,
        track_table: TrackTableSettings,
    ) -> SettingsSnapshot {
        let snapshot = self.settings.update_track_table(track_table).await;
        self.emit(AppEvent::Settings(snapshot.clone()));
        snapshot
    }

    pub async fn update_album_track_table_settings(
        &self,
        album_track_table: TrackTableSettings,
    ) -> SettingsSnapshot {
        let snapshot = self
            .settings
            .update_album_track_table(album_track_table)
            .await;
        self.emit(AppEvent::Settings(snapshot.clone()));
        snapshot
    }

    pub async fn update_playlist_track_table_settings(
        &self,
        playlist_track_table: TrackTableSettings,
    ) -> SettingsSnapshot {
        let snapshot = self
            .settings
            .update_playlist_track_table(playlist_track_table)
            .await;
        self.emit(AppEvent::Settings(snapshot.clone()));
        snapshot
    }

    pub async fn update_playback_preferences(
        &self,
        playback: PlaybackPreferences,
    ) -> Result<SettingsSnapshot, AppCoreError> {
        let playback_snapshot = self.playback.update_preferences(playback.clone()).await?;
        let settings_snapshot = self.settings.update_playback(playback).await;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            playback_snapshot,
        )));
        self.emit(AppEvent::Settings(settings_snapshot.clone()));
        Ok(settings_snapshot)
    }

    pub async fn update_auto_sync_on_startup(&self, enabled: bool) -> SettingsSnapshot {
        let snapshot = self.settings.update_auto_sync_on_startup(enabled).await;
        self.emit(AppEvent::Settings(snapshot.clone()));
        snapshot
    }

    pub async fn update_remote_storage_only(&self, enabled: bool) -> SettingsSnapshot {
        let snapshot = self.settings.update_remote_storage_only(enabled).await;
        self.emit(AppEvent::Settings(snapshot.clone()));
        snapshot
    }

    fn emit(&self, event: AppEvent) {
        let _ = self.events.send(event);
    }

    fn spawn_persistence_worker(&self, database: AppDatabase) {
        let playback = self.playback.clone();
        let mut events = self.events.subscribe();
        std::thread::Builder::new()
            .name("aria-persistence".into())
            .spawn(move || {
                let mut last_playback_session: Option<PlaybackSessionSnapshot> = None;
                loop {
                    match events.blocking_recv() {
                        Ok(AppEvent::Library(LibraryEvent::SnapshotChanged(mut snapshot))) => {
                            snapshot.is_scanning = false;
                            if let Err(error) = database.save_library_snapshot(&snapshot) {
                                eprintln!("failed to persist library snapshot: {error}");
                            }
                        }
                        Ok(AppEvent::Settings(snapshot)) => {
                            if let Err(error) = database.save_settings_snapshot(&snapshot) {
                                eprintln!("failed to persist settings snapshot: {error}");
                            }
                        }
                        Ok(AppEvent::Playback(_)) => {
                            let session = playback.persisted_session();
                            if last_playback_session.as_ref() != Some(&session) {
                                if let Err(error) = database.save_playback_session(&session) {
                                    eprintln!("failed to persist playback session: {error}");
                                } else {
                                    last_playback_session = Some(session);
                                }
                            }
                        }
                        Ok(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(snapshot))) => {
                            if let Err(error) = database.save_playlist_snapshot(&snapshot) {
                                eprintln!("failed to persist playlist snapshot: {error}");
                            }
                        }
                        Ok(_) => {}
                        Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            })
            .expect("failed to spawn aria-persistence worker");
    }

    fn spawn_remote_sync_worker(&self) {
        let core = self.clone();
        let worker = async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
            let auto_sync = core.settings.snapshot().await.auto_sync_on_startup;
            if let Err(e) = core.sync_startup_remote_targets(auto_sync).await {
                tracing::warn!("Automatic remote targets startup sync encountered error: {e}");
            }
        };

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(worker);
        } else {
            std::thread::Builder::new()
                .name("aria-remote-sync".into())
                .spawn(move || {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("failed to create remote sync worker runtime");
                    runtime.block_on(worker);
                })
                .expect("failed to spawn aria-remote-sync worker");
        }
    }

    fn spawn_playback_publisher(&self) {
        let playback = self.playback.clone();
        let events = self.events.clone();
        let core = self.clone();

        std::thread::Builder::new()
            .name("aria-playback-publisher".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                    .expect("failed to create playback publisher runtime");

                runtime.block_on(async move {
                    let mut last_snapshot = playback.snapshot().await;
                    let resolving_index: Arc<tokio::sync::Mutex<Option<usize>>> =
                        Arc::new(tokio::sync::Mutex::new(None));

                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

                        let snapshot = playback.snapshot().await;
                        if snapshot != last_snapshot {
                            let track_changed = snapshot.current_track != last_snapshot.current_track;
                            last_snapshot = snapshot.clone();
                            let _ = events
                                .send(AppEvent::Playback(PlaybackEvent::SnapshotChanged(snapshot.clone())));
                            if track_changed {
                                core.trigger_sliding_window_prefetch().await;
                            }
                        }

                        if snapshot.status == PlaybackStatus::Buffering {
                            if let Some(current_idx) = snapshot.current_queue_index {
                                let mut resolving = resolving_index.lock().await;
                                if *resolving != Some(current_idx) {
                                    *resolving = Some(current_idx);
                                    let core_clone = core.clone();
                                    let resolving_clone = resolving_index.clone();
                                    tokio::spawn(async move {
                                        core_clone.resolve_and_resume_buffering_track(current_idx).await;
                                        let mut r = resolving_clone.lock().await;
                                        if *r == Some(current_idx) {
                                            *r = None;
                                        }
                                    });
                                }
                            }
                        } else {
                            let mut resolving = resolving_index.lock().await;
                            *resolving = None;
                        }
                    }
                });
            })
            .expect("failed to spawn aria-playback-publisher");
    }

    async fn resolve_and_resume_buffering_track(&self, index: usize) {
        let session = self.playback.persisted_session();
        let Some(mut req) = session.queue.get(index).cloned() else {
            return;
        };

        let window: Vec<String> = session
            .queue
            .iter()
            .skip(index)
            .take(5)
            .map(|r| r.queue_item.id.clone())
            .collect();

        match self.resolve_remote_track_if_needed(&mut req, &window).await {
            Ok(_) => {
                let current_snapshot = self.playback.snapshot().await;
                if current_snapshot.current_queue_index == Some(index)
                    && current_snapshot.status == PlaybackStatus::Buffering
                {
                    if let Ok(new_snapshot) = self.playback.play().await {
                        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(new_snapshot)));
                        self.trigger_sliding_window_prefetch().await;
                    }
                }
            }
            Err(err) => {
                eprintln!("[app-core] failed to resolve buffering track at index {index}: {err}");
                let mut stopped_snapshot = self.playback.snapshot().await;
                stopped_snapshot.status = PlaybackStatus::Stopped;
                self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(stopped_snapshot)));
            }
        }
    }

    fn spawn_output_device_monitor(&self) {
        #[cfg(target_os = "android")]
        {
            // On Android, audio routing is handled natively by the OS AudioPolicyManager.
            // Continuous polling of CPAL output devices queries AudioPolicyManager JNI,
            // which causes active AAudio stream interruptions and stutter.
            let _ = self;
        }

        #[cfg(not(target_os = "android"))]
        {
            let playback = self.playback.clone();
            let settings = self.settings.clone();
            let events = self.events.clone();

            std::thread::Builder::new()
                .name("aria-output-device-monitor".into())
                .spawn(move || {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_time()
                        .build()
                        .expect("failed to create output device monitor runtime");

                    runtime.block_on(async move {
                        let mut last_devices = playback.list_output_devices().unwrap_or_default();

                        loop {
                            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;

                            let devices = match playback.list_output_devices() {
                                Ok(devices) => devices,
                                Err(error) => {
                                    eprintln!("failed to list output devices: {error}");
                                    continue;
                                }
                            };

                            if devices != last_devices {
                                last_devices = devices.clone();
                                let _ = events.send(AppEvent::Playback(
                                    PlaybackEvent::OutputDevicesChanged(devices.clone()),
                                ));
                            }

                            match playback.handle_output_device_change(&devices).await {
                                Ok(Some(refresh)) => {
                                    let _ = events.send(AppEvent::Playback(
                                        PlaybackEvent::SnapshotChanged(
                                            refresh.playback_snapshot.clone(),
                                        ),
                                    ));

                                    if let Some(updated_preferences) =
                                        refresh.updated_preferences.clone()
                                    {
                                        let settings_snapshot =
                                            settings.update_playback(updated_preferences).await;
                                        let _ = events.send(AppEvent::Settings(settings_snapshot));
                                    }
                                }
                                Ok(None) => {}
                                Err(error) => {
                                    eprintln!("failed to refresh output device state: {error}");
                                }
                            }
                        }
                    });
                })
                .expect("failed to spawn aria-output-device-monitor");
        }
    }

    pub fn get_remote_targets(&self) -> Result<Vec<RemoteTarget>, AppCoreError> {
        Ok(self.database.load_remote_targets()?)
    }

    pub fn save_remote_target(&self, target: RemoteTarget) -> Result<Vec<RemoteTarget>, AppCoreError> {
        let is_enabled = target.is_enabled;
        let target_id = target.id.clone();
        self.database.save_remote_target(&target)?;
        let targets = self.database.load_remote_targets()?;
        let _ = self.events.send(AppEvent::Remote(RemoteEvent::TargetsChanged(targets.clone())));
        if is_enabled {
            let core = self.clone();
            let sync_fut = async move {
                let _ = core.sync_remote_target(&target_id).await;
            };
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(sync_fut);
            } else {
                std::thread::Builder::new()
                    .name("aria-remote-target-sync".into())
                    .spawn(move || {
                        if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                            rt.block_on(sync_fut);
                        }
                    })
                    .ok();
            }
        }
        Ok(targets)
    }

    pub async fn delete_remote_target(&self, target_id: &str) -> Result<Vec<RemoteTarget>, AppCoreError> {
        self.database.delete_remote_target(target_id)?;
        let updated_snapshot = self.library.remove_remote_tracks_for_target(target_id).await;
        let _ = self.events.send(AppEvent::Library(LibraryEvent::SnapshotChanged(updated_snapshot)));
        let targets = self.database.load_remote_targets()?;
        let _ = self.events.send(AppEvent::Remote(RemoteEvent::TargetsChanged(targets.clone())));
        Ok(targets)
    }

    pub async fn test_remote_target(&self, target_id: &str) -> Result<StorageStatus, AppCoreError> {
        let targets = self.database.load_remote_targets()?;
        let target = targets
            .into_iter()
            .find(|t| t.id == target_id)
            .ok_or_else(|| AppCoreError::Remote(format!("Target '{target_id}' not found")))?;
        let backend = create_backend_for_target(&target)?;
        Ok(backend.test_connection().await?)
    }

    pub async fn start_gdrive_auth_flow(
        &self,
        client_id: String,
        client_secret: Option<String>,
        target_name: Option<String>,
        root_folder_name: Option<String>,
        storage_limit_bytes: Option<u64>,
    ) -> Result<String, AppCoreError> {
        let auth_config = GoogleAuthConfig {
            client_id: client_id.clone(),
            client_secret: client_secret.clone(),
        };
        let flow = PendingAuthFlow::start(auth_config).await?;
        let auth_url = flow.authorization_url.clone();
        let mut status_rx = flow.subscribe_status();
        let events = self.events.clone();
        let database = self.database.clone();
        let flow_client_id = flow.config().client_id.clone();
        let flow_client_secret = flow.config().client_secret.clone();
        let target_name_saved = target_name.clone();
        let root_folder_saved = root_folder_name.clone();

        tokio::spawn(async move {
            loop {
                let current_status = status_rx.borrow().clone();
                if let Some(res) = current_status {
                    match res {
                        Ok(tokens) => {
                            // Automatically construct and persist the RemoteTarget immediately
                            // as soon as the OAuth callback succeeds, so Android app pauses/resumes
                            // or event races can never lose the authenticated target.
                            if let Some(ref name) = target_name_saved {
                                let folder = root_folder_saved
                                    .as_deref()
                                    .map(|s| s.trim())
                                    .filter(|s| !s.is_empty())
                                    .unwrap_or("Aria")
                                    .to_string();

                                let stored_cfg = GoogleDriveStoredConfig {
                                    client_id: flow_client_id.clone(),
                                    client_secret: flow_client_secret.clone(),
                                    access_token: Some(tokens.access_token.clone()),
                                    refresh_token: tokens.refresh_token.clone(),
                                    expires_at: Some(tokens.expires_at),
                                    root_folder_name: folder,
                                };

                                let target = RemoteTarget {
                                    id: format!("gdrive-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()),
                                    name: name.clone(),
                                    backend_type: RemoteBackendType::GoogleDrive,
                                    storage_limit_bytes,
                                    is_enabled: true,
                                    config_json: serde_json::to_string(&stored_cfg).unwrap_or_default(),
                                };

                                if let Err(e) = database.save_remote_target(&target) {
                                    eprintln!("[app-core] failed to auto-save Google Drive remote target: {e}");
                                } else {
                                    *GLOBAL_LAST_COMPLETED_GDRIVE_TARGET.lock().await = Some(target);
                                }
                            }

                            let _ = events.send(AppEvent::Remote(RemoteEvent::GoogleAuthCompleted(
                                aria_domain::GoogleAuthCompletedEvent {
                                    success: true,
                                    error_message: None,
                                },
                            )));
                        }
                        Err(err) => {
                            let _ = events.send(AppEvent::Remote(RemoteEvent::GoogleAuthCompleted(
                                aria_domain::GoogleAuthCompletedEvent {
                                    success: false,
                                    error_message: Some(err.clone()),
                                },
                            )));
                        }
                    }
                    break;
                }
                if status_rx.changed().await.is_err() {
                    break;
                }
            }
        });

        *GLOBAL_PENDING_GDRIVE_AUTH.lock().await = Some(flow);
        *GLOBAL_LAST_COMPLETED_GDRIVE_TARGET.lock().await = None;
        Ok(auth_url)
    }

    pub async fn complete_gdrive_auth_flow(
        &self,
        target_name: String,
        root_folder_name: Option<String>,
        storage_limit_bytes: Option<u64>,
    ) -> Result<RemoteTarget, AppCoreError> {
        // 1. If target was already auto-saved when callback succeeded, return it immediately!
        {
            let last = GLOBAL_LAST_COMPLETED_GDRIVE_TARGET.lock().await;
            if let Some(ref target) = *last {
                return Ok(target.clone());
            }
        }

        // 2. Otherwise complete using the pending flow
        let flow_opt = {
            let mut pending = GLOBAL_PENDING_GDRIVE_AUTH.lock().await;
            pending.take()
        };

        let flow = match flow_opt {
            Some(f) => f,
            None => {
                // Check if target with this name or any gdrive target already exists in database
                if let Ok(targets) = self.database.load_remote_targets() {
                    if let Some(existing) = targets.into_iter().find(|t| {
                        t.backend_type == RemoteBackendType::GoogleDrive
                            && (t.name == target_name || target_name.is_empty())
                    }) {
                        return Ok(existing);
                    }
                }
                let last = GLOBAL_LAST_COMPLETED_GDRIVE_TARGET.lock().await;
                if let Some(ref target) = *last {
                    return Ok(target.clone());
                }
                return Err(AppCoreError::Remote("No pending Google Drive auth flow".into()));
            }
        };

        let client_id = flow.config().client_id.clone();
        let client_secret = flow.config().client_secret.clone();
        let tokens = flow.wait_for_tokens().await?;
        let root_folder = root_folder_name
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Aria".to_string());

        let stored_cfg = GoogleDriveStoredConfig {
            client_id,
            client_secret,
            access_token: Some(tokens.access_token),
            refresh_token: tokens.refresh_token,
            expires_at: Some(tokens.expires_at),
            root_folder_name: root_folder,
        };

        let target = RemoteTarget {
            id: format!("gdrive-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()),
            name: target_name,
            backend_type: RemoteBackendType::GoogleDrive,
            storage_limit_bytes,
            is_enabled: true,
            config_json: serde_json::to_string(&stored_cfg).map_err(|e| AppCoreError::Remote(e.to_string()))?,
        };

        self.save_remote_target(target.clone())?;

        let mut last = GLOBAL_LAST_COMPLETED_GDRIVE_TARGET.lock().await;
        *last = Some(target.clone());

        Ok(target)
    }

    pub async fn upload_album_to_target(
        &self,
        album_title: String,
        target_id: String,
    ) -> Result<(), AppCoreError> {
        let targets = self.database.load_remote_targets()?;
        let target = targets
            .into_iter()
            .find(|t| t.id == target_id)
            .ok_or_else(|| AppCoreError::Remote(format!("Target '{target_id}' not found")))?;

        let snapshot = self.library.snapshot().await;
        let album_tracks: Vec<ScannedTrack> = snapshot
            .tracks
            .into_iter()
            .filter(|t| {
                let title = t
                    .mapped_fields
                    .get("album")
                    .and_then(|a| a.first())
                    .map(|s| s.as_str())
                    .unwrap_or(&t.file_name);
                title.trim().eq_ignore_ascii_case(album_title.trim())
            })
            .collect();

        if album_tracks.is_empty() {
            return Err(AppCoreError::Remote(format!("No tracks found for album '{album_title}'")));
        }

        let backend = create_backend_for_target(&target)?;
        let current_usage = self.database.get_total_synced_bytes_for_target(&target_id)?;

        let (progress_tx, mut progress_rx) = broadcast::channel(64);
        let events = self.events.clone();

        tokio::spawn(async move {
            while let Ok(progress) = progress_rx.recv().await {
                let _ = events.send(AppEvent::Remote(RemoteEvent::UploadProgress(progress)));
            }
        });

        let core = self.clone();
        tokio::spawn(async move {
            let options = UploadOptions {
                compute_checksums: false,
                current_target_usage_bytes: current_usage,
            };

            match upload_album(
                backend,
                &target,
                &album_title,
                &album_tracks,
                options,
                Some(progress_tx),
            )
            .await
            {
                Ok(result) => {
                    let _ = core.database.save_remote_synced_items_batch(&result.synced_items);
                    let album_id = result
                        .synced_items
                        .iter()
                        .find(|i| i.item_type == "album")
                        .map(|i| i.item_id.clone())
                        .unwrap_or_default();
                    let has_failures = !result.failed_items.is_empty();
                    let _ = core.events.send(AppEvent::Remote(RemoteEvent::UploadCompleted(
                        UploadCompletedEvent {
                            target_id: target.id.clone(),
                            album_id,
                            album_title: album_title.clone(),
                            success: !has_failures,
                            error_message: if has_failures {
                                Some(format!("{} track(s) failed during upload", result.failed_items.len()))
                            } else {
                                None
                            },
                            failed_items: result.failed_items,
                            new_tracks_count: result.new_tracks_uploaded,
                            already_synced_tracks_count: result.already_synced_tracks,
                        },
                    )));
                }
                Err(e) => {
                    let mut failed_items = Vec::new();
                    for t in &album_tracks {
                        let title = t
                            .mapped_fields
                            .get("title")
                            .and_then(|v| v.first())
                            .cloned()
                            .unwrap_or_else(|| t.file_name.clone());
                        failed_items.push(UploadFailedItem {
                            item_id: Some(t.id.clone()),
                            title,
                            path: t.path.clone(),
                            error: e.to_string(),
                        });
                    }
                    let _ = core.events.send(AppEvent::Remote(RemoteEvent::UploadCompleted(
                        UploadCompletedEvent {
                            target_id: target.id.clone(),
                            album_id: String::new(),
                            album_title: album_title.clone(),
                            success: false,
                            error_message: Some(e.to_string()),
                            failed_items,
                            new_tracks_count: 0,
                            already_synced_tracks_count: 0,
                        },
                    )));
                }
            }
        });

        Ok(())
    }

    pub async fn upload_playlist_to_target(
        &self,
        playlist_id: String,
        target_id: String,
        upload_full_albums: bool,
    ) -> Result<(), AppCoreError> {
        let targets = self.database.load_remote_targets()?;
        let target = targets
            .into_iter()
            .find(|t| t.id == target_id)
            .ok_or_else(|| AppCoreError::Remote(format!("Target '{target_id}' not found")))?;

        let playlist_snapshot = self.playlists.snapshot().await;
        let playlist = playlist_snapshot
            .playlists
            .into_iter()
            .find(|p| p.id == playlist_id)
            .ok_or_else(|| AppCoreError::Playlist(PlaylistError::PlaylistNotFound))?;

        let snapshot = self.library.snapshot().await;
        let track_map: std::collections::HashMap<String, ScannedTrack> = snapshot
            .tracks
            .iter()
            .map(|t| (t.id.clone(), t.clone()))
            .collect();

        let playlist_tracks: Vec<ScannedTrack> = playlist
            .track_ids
            .iter()
            .filter_map(|id| track_map.get(id).cloned())
            .collect();

        if playlist_tracks.is_empty() {
            return Err(AppCoreError::Remote(format!(
                "No tracks found in library for playlist '{}'",
                playlist.name
            )));
        }

        let album_groups = group_playlist_tracks_by_album(
            &playlist_tracks,
            &snapshot.tracks,
            upload_full_albums,
        );

        if album_groups.is_empty() {
            return Err(AppCoreError::Remote(format!(
                "No album tracks found for playlist '{}'",
                playlist.name
            )));
        }

        let backend = create_backend_for_target(&target)?;
        let mut current_usage = self.database.get_total_synced_bytes_for_target(&target_id)?;

        let (progress_tx, mut progress_rx) = broadcast::channel(64);
        let events = self.events.clone();

        tokio::spawn(async move {
            while let Ok(progress) = progress_rx.recv().await {
                let _ = events.send(AppEvent::Remote(RemoteEvent::UploadProgress(progress)));
            }
        });

        let core = self.clone();
        let playlist_clone = playlist.clone();
        let playlist_name = playlist.name.clone();
        let playlist_id_clone = playlist.id.clone();

        tokio::spawn(async move {
            let mut all_failed_items: Vec<UploadFailedItem> = Vec::new();
            let mut successful_synced_track_ids: HashSet<String> = HashSet::new();
            let mut track_info_map: std::collections::HashMap<String, RemotePlaylistTrack> =
                std::collections::HashMap::new();
            let mut total_new_tracks_uploaded = 0usize;
            let mut total_already_synced_tracks = 0usize;

            let total_albums = album_groups.len();
            for (album_idx, (album_title, tracks_to_upload)) in album_groups.into_iter().enumerate() {
                let options = UploadOptions {
                    compute_checksums: false,
                    current_target_usage_bytes: current_usage,
                };

                // Pre-compute remote track paths and canonical track IDs for playlist manifest
                if let Ok(manifest) = build_album_manifest(&album_title, &tracks_to_upload, None, false) {
                    let relative_folder = derive_systematic_folder_name(&manifest);
                    for (idx, m_track) in manifest.tracks.iter().enumerate() {
                        if let Some(orig_track) = tracks_to_upload.get(idx) {
                            let (track_id, rel_path) = if orig_track.path.starts_with(&format!("remote://{}/", target.id)) {
                                (
                                    orig_track.id.clone(),
                                    orig_track
                                        .path
                                        .strip_prefix(&format!("remote://{}/", target.id))
                                        .unwrap_or(&orig_track.path)
                                        .to_string(),
                                )
                            } else {
                                (
                                    m_track.track_id.clone(),
                                    format!("{}/{}", relative_folder, m_track.relative_file_name),
                                )
                            };

                            track_info_map.insert(
                                orig_track.id.clone(),
                                RemotePlaylistTrack {
                                    track_id,
                                    relative_path: rel_path,
                                    title: orig_track
                                        .mapped_fields
                                        .get("title")
                                        .and_then(|t| t.first())
                                        .cloned()
                                        .unwrap_or_else(|| orig_track.file_name.clone()),
                                    composer: orig_track
                                        .mapped_fields
                                        .get("composer")
                                        .and_then(|t| t.first())
                                        .cloned(),
                                    album: orig_track
                                        .mapped_fields
                                        .get("album")
                                        .and_then(|t| t.first())
                                        .cloned(),
                                    duration_ms: orig_track.audio.duration_ms,
                                },
                            );
                        }
                    }
                }

                // Tag progress events with overall playlist album progress
                let (album_progress_tx, mut album_progress_rx) = broadcast::channel::<UploadProgressEvent>(64);
                let playlist_progress_tx = progress_tx.clone();
                let album_display_title = if total_albums > 1 {
                    format!("Playlist \"{}\" ({}/{})", playlist_name, album_idx + 1, total_albums)
                } else {
                    format!("Playlist \"{}\"", playlist_name)
                };
                let album_display_clone = album_display_title.clone();
                let album_title_clone = album_title.clone();

                tokio::spawn(async move {
                    while let Ok(mut progress) = album_progress_rx.recv().await {
                        progress.album_title = album_display_clone.clone();
                        if progress.current_file == "Done" || progress.current_file == "All tracks verified" {
                            progress.current_file = album_title_clone.clone();
                        }
                        let _ = playlist_progress_tx.send(progress);
                    }
                });

                match upload_album(
                    backend.clone(),
                    &target,
                    &album_title,
                    &tracks_to_upload,
                    options,
                    Some(album_progress_tx),
                )
                .await
                {
                    Ok(result) => {
                        for item in &result.synced_items {
                            if item.item_type == "track" {
                                successful_synced_track_ids.insert(item.item_id.clone());
                            }
                        }
                        let _ = core.database.save_remote_synced_items_batch(&result.synced_items);
                        if let Ok(usage) = core.database.get_total_synced_bytes_for_target(&target.id) {
                            current_usage = usage;
                        }
                        total_new_tracks_uploaded += result.new_tracks_uploaded;
                        total_already_synced_tracks += result.already_synced_tracks;
                        all_failed_items.extend(result.failed_items);
                    }
                    Err(e) => {
                        for t in &tracks_to_upload {
                            let title = t
                                .mapped_fields
                                .get("title")
                                .and_then(|v| v.first())
                                .cloned()
                                .unwrap_or_else(|| t.file_name.clone());
                            all_failed_items.push(UploadFailedItem {
                                item_id: Some(t.id.clone()),
                                title,
                                path: t.path.clone(),
                                error: e.to_string(),
                            });
                        }
                    }
                }
            }

            // Upload the playlist manifest (.json) and portable playlist (.m3u8) to remote with available tracks
            let mut playlist_remote_tracks = Vec::new();
            for track_id in &playlist_clone.track_ids {
                if let Some(rpt) = track_info_map.get(track_id) {
                    if successful_synced_track_ids.contains(&rpt.track_id)
                        || playlist_tracks.iter().any(|pt| pt.id == *track_id && pt.path.starts_with(&format!("remote://{}/", target.id)))
                    {
                        playlist_remote_tracks.push(rpt.clone());
                    }
                }
            }

            if !playlist_remote_tracks.is_empty() {
                match upload_playlist(
                    backend.clone(),
                    &target,
                    &playlist_clone,
                    &playlist_remote_tracks,
                    Some(progress_tx.clone()),
                )
                .await
                {
                    Ok(synced_item) => {
                        let _ = core.database.save_remote_synced_item(&synced_item);
                    }
                    Err(e) => {
                        all_failed_items.push(UploadFailedItem {
                            item_id: Some(playlist_id_clone.clone()),
                            title: format!("Playlist \"{}\"", playlist_name),
                            path: format!("playlists/{}.json", playlist_id_clone),
                            error: format!("Failed to upload playlist manifest: {e}"),
                        });
                    }
                }
            }

            let has_failures = !all_failed_items.is_empty();
            let _ = core.events.send(AppEvent::Remote(RemoteEvent::UploadCompleted(
                UploadCompletedEvent {
                    target_id: target.id.clone(),
                    album_id: playlist_id_clone,
                    album_title: format!("Playlist \"{}\"", playlist_name),
                    success: !has_failures,
                    error_message: if has_failures {
                        Some(format!("{} track(s) failed during upload", all_failed_items.len()))
                    } else {
                        None
                    },
                    failed_items: all_failed_items,
                    new_tracks_count: total_new_tracks_uploaded,
                    already_synced_tracks_count: total_already_synced_tracks,
                },
            )));
        });

        Ok(())
    }

    pub async fn delete_album_from_target(
        &self,
        album_id: &str,
        target_id: &str,
    ) -> Result<(), AppCoreError> {
        let targets = self.database.load_remote_targets()?;
        let target = targets
            .into_iter()
            .find(|t| t.id == target_id)
            .ok_or_else(|| AppCoreError::Remote(format!("Target '{target_id}' not found")))?;

        let synced_items = self.database.load_remote_synced_items_for_target(target_id)?;
        if let Some(album_item) = synced_items.iter().find(|i| i.item_id == album_id && i.item_type == "album") {
            let backend = create_backend_for_target(&target)?;
            let _ = backend.delete_directory(&album_item.remote_path).await;
        }

        self.database.delete_remote_synced_item(album_id, target_id)?;
        for item in synced_items {
            if item.item_type == "track" {
                let _ = self.database.delete_remote_synced_item(&item.item_id, target_id);
            }
        }

        Ok(())
    }

    pub async fn delete_playlist_from_target(
        &self,
        playlist_id: &str,
        target_id: &str,
    ) -> Result<(), AppCoreError> {
        let targets = self.database.load_remote_targets()?;
        let target = targets
            .into_iter()
            .find(|t| t.id == target_id)
            .ok_or_else(|| AppCoreError::Remote(format!("Target '{target_id}' not found")))?;

        let backend = create_backend_for_target(&target)?;
        let manifest_path = format!("playlists/{playlist_id}.json");
        let _ = backend.delete_file(&manifest_path).await;

        // Also update aria-playlists-index.json
        if let Ok(bytes) = backend.read_bytes("aria-playlists-index.json").await {
            let json = String::from_utf8_lossy(&bytes);
            if let Ok(mut index) = aria_library::manifest::deserialize_master_playlist_index(&json) {
                if let Some(removed) = index.playlists.iter().find(|p| p.playlist_id == playlist_id) {
                    if let Some(ref m3u) = removed.m3u_path {
                        let _ = backend.delete_file(m3u).await;
                    }
                }
                index.playlists.retain(|p| p.playlist_id != playlist_id);
                index.updated_at = chrono::Utc::now().to_rfc3339();
                if let Ok(serialized) = aria_library::manifest::serialize_master_playlist_index(&index) {
                    let _ = backend.write_bytes("aria-playlists-index.json", serialized.as_bytes()).await;
                }
            }
        }

        self.database.delete_remote_synced_item(playlist_id, target_id)?;
        Ok(())
    }

    pub fn get_remote_synced_items(&self, target_id: &str) -> Result<Vec<RemoteSyncedItem>, AppCoreError> {
        Ok(self.database.load_remote_synced_items_for_target(target_id)?)
    }

    pub async fn get_remote_cache_status(&self) -> RemoteCacheStatus {
        self.cache_manager.get_status().await
    }

    pub async fn clear_remote_cache(&self) -> Result<(), AppCoreError> {
        self.cache_manager.clear().await?;
        Ok(())
    }

    pub async fn sync_remote_target(
        &self,
        target_id: &str,
    ) -> Result<RemoteSyncCompletedEvent, AppCoreError> {
        let targets = self.database.load_remote_targets()?;
        let target = targets
            .into_iter()
            .find(|t| t.id == target_id)
            .ok_or_else(|| AppCoreError::Remote(format!("Target '{target_id}' not found")))?;

        if !target.is_enabled {
            return Err(AppCoreError::Remote(format!(
                "Target '{}' is disabled",
                target.name
            )));
        }

        let remote_storage_only = self.settings.snapshot().await.remote_storage_only;
        if !remote_storage_only {
            let backend = create_backend_for_target(&target)?;
            let mut current_usage = self.database.get_total_synced_bytes_for_target(&target.id)?;

            let library_snapshot = self.library.snapshot().await;
            let local_tracks: Vec<ScannedTrack> = library_snapshot
                .tracks
                .into_iter()
                .filter(|t| !t.path.starts_with("remote://"))
                .collect();

            let album_groups = group_local_tracks_by_album(&local_tracks);
            let total_albums = album_groups.len();

            let (progress_tx, mut progress_rx) = broadcast::channel(64);
            let events = self.events.clone();

            tokio::spawn(async move {
                while let Ok(progress) = progress_rx.recv().await {
                    let _ = events.send(AppEvent::Remote(RemoteEvent::SyncProgress(progress)));
                }
            });

            let mut total_new_tracks_uploaded = 0usize;
            let mut total_new_albums_uploaded = 0usize;
            let mut had_error = false;
            let mut last_error_msg = None;

            for (album_idx, (album_title, tracks_to_upload)) in album_groups.into_iter().enumerate() {
                let _ = progress_tx.send(RemoteSyncProgressEvent {
                    target_id: target.id.clone(),
                    target_name: target.name.clone(),
                    current_album: format!("{album_title} ({}/{})", album_idx + 1, total_albums),
                    completed_albums: album_idx,
                    total_albums,
                    status: "uploading_album".to_string(),
                });

                let options = UploadOptions {
                    compute_checksums: false,
                    current_target_usage_bytes: current_usage,
                };

                let (album_progress_tx, mut album_progress_rx) = broadcast::channel::<UploadProgressEvent>(32);
                let target_id_clone = target.id.clone();
                let target_name_clone = target.name.clone();
                let album_title_display = album_title.clone();
                let progress_tx_clone = progress_tx.clone();

                tokio::spawn(async move {
                    while let Ok(upload_prog) = album_progress_rx.recv().await {
                        let _ = progress_tx_clone.send(RemoteSyncProgressEvent {
                            target_id: target_id_clone.clone(),
                            target_name: target_name_clone.clone(),
                            current_album: format!("{}: {}", album_title_display, upload_prog.current_file),
                            completed_albums: album_idx,
                            total_albums,
                            status: upload_prog.status,
                        });
                    }
                });

                match upload_album(
                    backend.clone(),
                    &target,
                    &album_title,
                    &tracks_to_upload,
                    options,
                    Some(album_progress_tx),
                )
                .await
                {
                    Ok(result) => {
                        if !result.synced_items.is_empty() {
                            let _ = self.database.save_remote_synced_items_batch(&result.synced_items);
                        }
                        if let Ok(usage) = self.database.get_total_synced_bytes_for_target(&target.id) {
                            current_usage = usage;
                        }
                        total_new_tracks_uploaded += result.new_tracks_uploaded;
                        if result.new_tracks_uploaded > 0 {
                            total_new_albums_uploaded += 1;
                        }
                    }
                    Err(e) => {
                        had_error = true;
                        last_error_msg = Some(e.to_string());
                        tracing::warn!("Failed to upload album '{album_title}' during local-to-remote sync: {e}");
                    }
                }
            }

            let playlists_snapshot = self.playlists.snapshot().await;
            let mut total_new_playlists = 0usize;
            for playlist in &playlists_snapshot.playlists {
                let playlist_tracks: Vec<ScannedTrack> = playlist
                    .track_ids
                    .iter()
                    .filter_map(|id| local_tracks.iter().find(|t| &t.id == id).cloned())
                    .collect();
                if playlist_tracks.is_empty() {
                    continue;
                }
                let mut r_tracks = Vec::new();
                for pt in &playlist_tracks {
                    let alb_title = pt
                        .mapped_fields
                        .get("album")
                        .and_then(|a| a.first())
                        .map(|s| s.as_str())
                        .unwrap_or(&pt.file_name);
                    if let Ok(manifest) = build_album_manifest(alb_title, &[pt.clone()], None, false) {
                        let folder = derive_systematic_folder_name(&manifest);
                        let m_track = manifest.tracks.first();
                        let rel_name = m_track.map(|m| m.relative_file_name.as_str()).unwrap_or(&pt.file_name);
                        r_tracks.push(RemotePlaylistTrack {
                            track_id: pt.id.clone(),
                            relative_path: format!("{}/{}", folder, rel_name),
                            title: pt.mapped_fields.get("title").and_then(|t| t.first()).cloned().unwrap_or_else(|| pt.file_name.clone()),
                            composer: pt.mapped_fields.get("composer").and_then(|c| c.first()).cloned(),
                            album: Some(alb_title.to_string()),
                            duration_ms: pt.audio.duration_ms,
                        });
                    }
                }
                if !r_tracks.is_empty() {
                    if let Ok(synced_item) = upload_playlist(backend.clone(), &target, playlist, &r_tracks, None).await {
                        let _ = self.database.save_remote_synced_item(&synced_item);
                        total_new_playlists += 1;
                    }
                }
            }

            let now = chrono::Utc::now().to_rfc3339();
            let _ = self.database.save_remote_target_last_sync(&target.id, &now);

            let _ = progress_tx.send(RemoteSyncProgressEvent {
                target_id: target.id.clone(),
                target_name: target.name.clone(),
                current_album: String::new(),
                completed_albums: total_albums,
                total_albums,
                status: "completed".to_string(),
            });

            let completed_event = RemoteSyncCompletedEvent {
                target_id: target.id.clone(),
                target_name: target.name.clone(),
                new_albums_count: total_new_albums_uploaded,
                new_tracks_count: total_new_tracks_uploaded,
                new_playlists_count: total_new_playlists,
                success: !had_error || total_new_tracks_uploaded > 0 || total_albums == 0,
                error_message: if had_error && total_new_tracks_uploaded == 0 && total_albums > 0 {
                    last_error_msg
                } else {
                    None
                },
            };
            let _ = self.events.send(AppEvent::Remote(RemoteEvent::SyncCompleted(
                completed_event.clone(),
            )));
            return Ok(completed_event);
        }

        let backend = create_backend_for_target(&target)?;
        let artwork_cache_dir = self.cache_manager.cache_dir().join("artwork");

        let library_snapshot = self.library.snapshot().await;
        let existing_ids: HashSet<String> = library_snapshot
            .tracks
            .iter()
            .filter(|t| t.path.starts_with("remote://"))
            .map(|t| t.id.clone())
            .collect();

        let target_last_synced_at = self.database.get_remote_target_last_sync(&target.id)?;
        let existing_album_sync_times = self
            .database
            .load_synced_album_timestamps_for_target(&target.id)?;

        let (progress_tx, mut progress_rx) = broadcast::channel(64);
        let events = self.events.clone();

        tokio::spawn(async move {
            while let Ok(progress) = progress_rx.recv().await {
                let _ = events.send(AppEvent::Remote(RemoteEvent::SyncProgress(progress)));
            }
        });

        let backend_for_playlists = backend.clone();
        let sync_progress_tx = progress_tx.clone();
        match sync_remote_library(
            backend,
            &target,
            &artwork_cache_dir,
            &existing_ids,
            target_last_synced_at.as_deref(),
            &existing_album_sync_times,
            Some(progress_tx),
        )
        .await
        {
            Ok(result) => {
                // Save synced items to database in a single batch
                if !result.synced_items.is_empty() {
                    let _ = sync_progress_tx.send(RemoteSyncProgressEvent {
                        target_id: target.id.clone(),
                        target_name: target.name.clone(),
                        current_album: "Saving to database...".to_string(),
                        completed_albums: 0,
                        total_albums: 0,
                        status: "saving_database".to_string(),
                    });
                    let _ = self
                        .database
                        .save_remote_synced_items_batch(&result.synced_items);
                }

                // Update target last sync timestamp from master index
                if let Some(ref updated_at) = result.master_index_updated_at {
                    let _ = self
                        .database
                        .save_remote_target_last_sync(&target.id, updated_at);
                }

                // Merge synced tracks into the library
                if !result.synced_tracks.is_empty() {
                    let _ = sync_progress_tx.send(RemoteSyncProgressEvent {
                        target_id: target.id.clone(),
                        target_name: target.name.clone(),
                        current_album: "Updating local library...".to_string(),
                        completed_albums: 0,
                        total_albums: 0,
                        status: "updating_library".to_string(),
                    });
                    let updated_snapshot = self
                        .library
                        .merge_remote_tracks(result.synced_tracks)
                        .await;
                    let _ = self.events.send(AppEvent::Library(LibraryEvent::SnapshotChanged(
                        updated_snapshot,
                    )));
                }

                // Sync playlists from remote target
                let _ = sync_progress_tx.send(RemoteSyncProgressEvent {
                    target_id: target.id.clone(),
                    target_name: target.name.clone(),
                    current_album: "Syncing playlists from remote...".to_string(),
                    completed_albums: 0,
                    total_albums: 0,
                    status: "syncing_playlists".to_string(),
                });
                let mut new_playlists_count = 0;
                if let Ok(remote_playlists) = sync_remote_playlists(backend_for_playlists, &target).await {
                    let library_snapshot = self.library.snapshot().await;
                    let current_playlists = self.playlists.snapshot().await;
                    let mut playlist_changed = false;

                    for remote_pl in remote_playlists {
                        let mut resolved_track_ids = Vec::new();
                        for rpt in &remote_pl.tracks {
                            // 1. Direct match by canonical track ID
                            if let Some(track) = library_snapshot.tracks.iter().find(|t| t.id == rpt.track_id) {
                                resolved_track_ids.push(track.id.clone());
                                continue;
                            }
                            // 2. Match by remote path suffix
                            let remote_suffix = format!("{}/{}", target.id, rpt.relative_path);
                            if let Some(track) = library_snapshot.tracks.iter().find(|t| t.path.ends_with(&remote_suffix)) {
                                resolved_track_ids.push(track.id.clone());
                                continue;
                            }
                            // 3. Match by mapped title and album
                            if let Some(track) = library_snapshot.tracks.iter().find(|t| {
                                let title_match = t
                                    .mapped_fields
                                    .get("title")
                                    .and_then(|v| v.first())
                                    .map(|s| s.as_str())
                                    .unwrap_or(&t.file_name)
                                    .eq_ignore_ascii_case(&rpt.title);
                                let album_match = if let Some(ref alb) = rpt.album {
                                    t.mapped_fields
                                        .get("album")
                                        .and_then(|v| v.first())
                                        .map(|s| s.as_str())
                                        .unwrap_or("")
                                        .eq_ignore_ascii_case(alb)
                                } else {
                                    true
                                };
                                title_match && album_match
                            }) {
                                resolved_track_ids.push(track.id.clone());
                            }
                        }

                        let playlist_synced_item = RemoteSyncedItem {
                            item_id: remote_pl.playlist_id.clone(),
                            remote_target_id: target.id.clone(),
                            item_type: "playlist".to_string(),
                            remote_path: format!("playlists/{}.json", remote_pl.playlist_id),
                            remote_file_id: None,
                            size_bytes: 0,
                            checksum: None,
                            sync_status: "synced".to_string(),
                            last_synced_at: remote_pl.updated_at.clone(),
                        };
                        let _ = self.database.save_remote_synced_item(&playlist_synced_item);

                        if let Some(existing) = current_playlists.playlists.iter().find(|p| p.id == remote_pl.playlist_id) {
                            if existing.track_ids.is_empty() && !resolved_track_ids.is_empty() {
                                let _ = self.playlists.add_tracks(existing.id.clone(), resolved_track_ids).await;
                                playlist_changed = true;
                            }
                        } else {
                            let _ = self.playlists.import_synced_playlist(Playlist {
                                id: remote_pl.playlist_id.clone(),
                                name: remote_pl.name.clone(),
                                collage_seed: remote_pl.collage_seed,
                                track_ids: resolved_track_ids,
                                created_at: remote_pl.created_at.clone(),
                            }).await;
                            new_playlists_count += 1;
                            playlist_changed = true;
                        }
                    }

                    if playlist_changed {
                        let updated_playlists = self.playlists.snapshot().await;
                        let _ = self.events.send(AppEvent::Playlists(PlaylistEvent::SnapshotChanged(
                            updated_playlists,
                        )));
                    }
                }

                let _ = sync_progress_tx.send(RemoteSyncProgressEvent {
                    target_id: target.id.clone(),
                    target_name: target.name.clone(),
                    current_album: String::new(),
                    completed_albums: 0,
                    total_albums: 0,
                    status: "completed".to_string(),
                });

                let completed = RemoteSyncCompletedEvent {
                    target_id: target.id.clone(),
                    target_name: target.name.clone(),
                    new_albums_count: result.new_albums_count,
                    new_tracks_count: result.new_tracks_count,
                    new_playlists_count,
                    success: true,
                    error_message: None,
                };
                let _ = self.events.send(AppEvent::Remote(RemoteEvent::SyncCompleted(
                    completed.clone(),
                )));
                Ok(completed)
            }
            Err(err) => {
                let completed = RemoteSyncCompletedEvent {
                    target_id: target.id.clone(),
                    target_name: target.name.clone(),
                    new_albums_count: 0,
                    new_tracks_count: 0,
                    new_playlists_count: 0,
                    success: false,
                    error_message: Some(err.to_string()),
                };
                let _ = self.events.send(AppEvent::Remote(RemoteEvent::SyncCompleted(
                    completed.clone(),
                )));
                Err(AppCoreError::RemoteStorage(err))
            }
        }
    }

    pub async fn sync_all_remote_targets(
        &self,
    ) -> Result<Vec<RemoteSyncCompletedEvent>, AppCoreError> {
        let targets = self.database.load_remote_targets()?;
        let mut results = Vec::new();
        for target in targets {
            if target.is_enabled {
                match self.sync_remote_target(&target.id).await {
                    Ok(event) => results.push(event),
                    Err(e) => {
                        tracing::warn!("Failed to sync remote target '{}': {e}", target.name);
                        results.push(RemoteSyncCompletedEvent {
                            target_id: target.id.clone(),
                            target_name: target.name.clone(),
                            new_albums_count: 0,
                            new_tracks_count: 0,
                            new_playlists_count: 0,
                            success: false,
                            error_message: Some(e.to_string()),
                        });
                    }
                }
            }
        }
        Ok(results)
    }

    pub async fn sync_startup_remote_targets(
        &self,
        force_auto_sync: bool,
    ) -> Result<Vec<RemoteSyncCompletedEvent>, AppCoreError> {
        let targets = self.database.load_remote_targets()?;
        let mut results = Vec::new();
        for target in targets {
            if !target.is_enabled {
                continue;
            }

            let has_synced_previously = self
                .database
                .get_remote_target_last_sync(&target.id)?
                .is_some()
                || !self
                    .database
                    .load_remote_synced_items_for_target(&target.id)?
                    .is_empty();

            if has_synced_previously && !force_auto_sync {
                tracing::info!(
                    "Skipping startup sync for target '{}' because it was already synced previously.",
                    target.name
                );
                continue;
            }

            match self.sync_remote_target(&target.id).await {
                Ok(event) => results.push(event),
                Err(e) => {
                    tracing::warn!("Failed to sync remote target '{}': {e}", target.name);
                    results.push(RemoteSyncCompletedEvent {
                        target_id: target.id.clone(),
                        target_name: target.name.clone(),
                        new_albums_count: 0,
                        new_tracks_count: 0,
                        new_playlists_count: 0,
                        success: false,
                        error_message: Some(e.to_string()),
                    });
                }
            }
        }
        Ok(results)
    }
}

fn sanitize_playlist_filename(name: &str) -> String {
    let cleaned = name
        .chars()
        .map(|character| match character {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ => character,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();

    if cleaned.is_empty() {
        "playlist".into()
    } else {
        cleaned
    }
}

fn clean_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut result = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                result.pop();
            }
            std::path::Component::CurDir => {}
            other => {
                result.push(other.as_os_str());
            }
        }
    }
    result
}

fn normalize_for_comparison(path: &std::path::Path) -> String {
    let cleaned = clean_path(path);
    cleaned.to_string_lossy().replace('\\', "/").to_lowercase()
}

pub fn fold_for_matching(s: &str) -> String {
    // Normalize well-known visual homoglyphs before deunicoding:
    // Greek beta '\u{03b2}' / '\u{03d0}' is frequently typed or CP437-converted in place of German sharp s 'ß' (U+00DF).
    let homoglyphs_normalized = s
        .replace('\u{03b2}', "ss")
        .replace('\u{03d0}', "ss")
        .replace('ß', "ss");
    deunicode::deunicode(&homoglyphs_normalized)
}

fn extract_track_number_prefix(filename: &str) -> Option<u32> {
    let trimmed = filename.trim_start();
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        let rest = &trimmed[digits.len()..];
        if rest.starts_with('.') || rest.starts_with('-') || rest.starts_with(' ') || rest.starts_with('_') {
            return digits.parse::<u32>().ok();
        }
    }
    None
}

pub struct PlaylistTrackMatcher<'a> {
    exact_path: std::collections::HashMap<String, &'a str>,
    folded_path: std::collections::HashMap<String, Vec<&'a str>>,
    by_filename: std::collections::HashMap<String, Vec<&'a ScannedTrack>>,
    by_folded_filename: std::collections::HashMap<String, Vec<&'a ScannedTrack>>,
    by_parent_folder: std::collections::HashMap<String, Vec<&'a ScannedTrack>>,
}

impl<'a> PlaylistTrackMatcher<'a> {
    pub fn new(tracks: &'a [ScannedTrack]) -> Self {
        let mut exact_path = std::collections::HashMap::with_capacity(tracks.len());
        let mut folded_path: std::collections::HashMap<String, Vec<&'a str>> =
            std::collections::HashMap::with_capacity(tracks.len());
        let mut by_filename: std::collections::HashMap<String, Vec<&'a ScannedTrack>> =
            std::collections::HashMap::with_capacity(tracks.len());
        let mut by_folded_filename: std::collections::HashMap<String, Vec<&'a ScannedTrack>> =
            std::collections::HashMap::with_capacity(tracks.len());
        let mut by_parent_folder: std::collections::HashMap<String, Vec<&'a ScannedTrack>> =
            std::collections::HashMap::new();

        for track in tracks {
            let norm = normalize_for_comparison(std::path::Path::new(&track.path));
            exact_path.insert(norm.clone(), track.id.as_str());

            let folded = fold_for_matching(&norm);
            folded_path.entry(folded).or_default().push(track.id.as_str());

            let track_p = std::path::Path::new(&track.path);
            if let Some(file_name) = track_p.file_name().and_then(|f| f.to_str()) {
                let fn_lower = file_name.to_lowercase();
                by_filename.entry(fn_lower.clone()).or_default().push(track);

                let fn_folded = fold_for_matching(&fn_lower);
                by_folded_filename.entry(fn_folded).or_default().push(track);
            }

            if let Some(parent_name) = track_p.parent().and_then(|p| p.file_name()).and_then(|f| f.to_str()) {
                let folder_folded = fold_for_matching(parent_name).to_lowercase();
                by_parent_folder.entry(folder_folded).or_default().push(track);
            }
        }

        Self {
            exact_path,
            folded_path,
            by_filename,
            by_folded_filename,
            by_parent_folder,
        }
    }

    pub fn find_match(&self, resolved: &std::path::Path) -> Option<String> {
        let norm = normalize_for_comparison(resolved);

        // Tier 1: Exact normalized path match
        if let Some(id) = self.exact_path.get(&norm) {
            return Some((*id).to_string());
        }

        // Tier 2: Diacritic / accent / homoglyph-folded path match
        let folded = fold_for_matching(&norm);
        if let Some(ids) = self.folded_path.get(&folded) {
            if ids.len() == 1 {
                return Some(ids[0].to_string());
            }
        }

        // Tier 3: Filename + Parent directory / Unique Filename match
        if let Some(file_name) = resolved.file_name().and_then(|f| f.to_str()) {
            let fn_lower = file_name.to_lowercase();
            let fn_folded = fold_for_matching(&fn_lower);
            let candidates = self
                .by_filename
                .get(&fn_lower)
                .or_else(|| self.by_folded_filename.get(&fn_folded));

            if let Some(candidates) = candidates {
                if candidates.len() == 1 {
                    return Some(candidates[0].id.clone());
                }

                // If multiple candidate tracks share this filename, match on parent folder name
                if let Some(playlist_parent) =
                    resolved.parent().and_then(|p| p.file_name()).and_then(|f| f.to_str())
                {
                    let folded_parent = fold_for_matching(playlist_parent).to_lowercase();
                    let parent_matches: Vec<&ScannedTrack> = candidates
                        .iter()
                        .copied()
                        .filter(|c| {
                            let track_path = std::path::Path::new(&c.path);
                            track_path
                                .parent()
                                .and_then(|p| p.file_name())
                                .and_then(|f| f.to_str())
                                .map(|name| fold_for_matching(name).to_lowercase() == folded_parent)
                                .unwrap_or(false)
                        })
                        .collect();

                    if parent_matches.len() == 1 {
                        return Some(parent_matches[0].id.clone());
                    }
                }
            }
        }

        // Tier 4: Parent Folder + Track Number Prefix Match
        // Handles cases where track titles in filenames had punctuation or minor differences
        if let Some(playlist_parent) =
            resolved.parent().and_then(|p| p.file_name()).and_then(|f| f.to_str())
        {
            let folded_parent = fold_for_matching(playlist_parent).to_lowercase();
            if let Some(folder_tracks) = self.by_parent_folder.get(&folded_parent) {
                if let Some(file_name) = resolved.file_name().and_then(|f| f.to_str()) {
                    if let Some(target_track_num) = extract_track_number_prefix(file_name) {
                        let num_matches: Vec<&ScannedTrack> = folder_tracks
                            .iter()
                            .copied()
                            .filter(|c| {
                                std::path::Path::new(&c.path)
                                    .file_name()
                                    .and_then(|f| f.to_str())
                                    .and_then(extract_track_number_prefix)
                                    == Some(target_track_num)
                            })
                            .collect();

                        if num_matches.len() == 1 {
                            return Some(num_matches[0].id.clone());
                        }
                    }
                }
            }
        }

        None
    }
}

#[cfg(target_os = "windows")]
extern "system" {
    fn GetACP() -> u32;
    fn MultiByteToWideChar(
        CodePage: u32,
        dwFlags: u32,
        lpMultiByteStr: *const u8,
        cbMultiByte: i32,
        lpWideCharStr: *mut u16,
        cchWideChar: i32,
    ) -> i32;
}

#[cfg(target_os = "windows")]
pub fn get_system_default_codepage() -> u32 {
    unsafe { GetACP() }
}

#[cfg(not(target_os = "windows"))]
pub fn get_system_default_codepage() -> u32 {
    65001 // UTF-8 fallback
}

#[cfg(target_os = "windows")]
fn decode_with_codepage(bytes: &[u8], codepage: u32) -> Result<String, String> {
    if bytes.is_empty() {
        return Ok(String::new());
    }

    let input_len = bytes.len() as i32;
    unsafe {
        let required_len = MultiByteToWideChar(
            codepage,
            0,
            bytes.as_ptr(),
            input_len,
            std::ptr::null_mut(),
            0,
        );

        if required_len <= 0 {
            return Err("Failed to convert string (length calculation)".to_string());
        }

        let mut buf = vec![0u16; required_len as usize];
        let result = MultiByteToWideChar(
            codepage,
            0,
            bytes.as_ptr(),
            input_len,
            buf.as_mut_ptr(),
            required_len,
        );

        if result <= 0 {
            return Err("Failed to convert string".to_string());
        }

        Ok(String::from_utf16_lossy(&buf))
    }
}

#[cfg(not(target_os = "windows"))]
fn decode_with_codepage(bytes: &[u8], _codepage: u32) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|err| err.to_string())
}

fn parse_playlist_content(content: &str, extension: &str) -> Vec<(String, Option<String>)> {
    let mut raw_paths = Vec::new();
    if extension == "pls" {
        let mut files = std::collections::BTreeMap::new();
        let mut titles = std::collections::BTreeMap::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(eq_idx) = trimmed.find('=') {
                let key = trimmed[..eq_idx].trim().to_ascii_lowercase();
                let val = trimmed[eq_idx + 1..].trim().to_string();
                if !val.is_empty() {
                    if key.starts_with("file") {
                        if let Ok(num) = key[4..].parse::<u32>() {
                            files.insert(num, val);
                        }
                    } else if key.starts_with("title") {
                        if let Ok(num) = key[5..].parse::<u32>() {
                            titles.insert(num, val);
                        }
                    }
                }
            }
        }
        for (num, file_path) in files {
            let title = titles.get(&num).cloned();
            raw_paths.push((file_path, title));
        }
    } else {
        // Default to M3U / M3U8 parsing
        let mut last_extinf = None;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with("#EXTINF:") {
                if let Some(comma_idx) = trimmed.find(',') {
                    last_extinf = Some(trimmed[comma_idx + 1..].trim().to_string());
                }
            } else if trimmed.starts_with('#') {
                continue;
            } else {
                raw_paths.push((trimmed.to_string(), last_extinf.take()));
            }
        }
    }
    raw_paths
}

pub fn group_playlist_tracks_by_album(
    playlist_tracks: &[ScannedTrack],
    all_library_tracks: &[ScannedTrack],
    upload_full_albums: bool,
) -> Vec<(String, Vec<ScannedTrack>)> {
    let get_album_title = |t: &ScannedTrack| -> String {
        t.mapped_fields
            .get("album")
            .and_then(|a| a.first())
            .map(|s| s.as_str())
            .unwrap_or(&t.file_name)
            .trim()
            .to_string()
    };

    let mut album_groups: Vec<(String, Vec<ScannedTrack>)> = Vec::new();
    let mut seen_albums = std::collections::HashSet::new();

    for track in playlist_tracks {
        let title = get_album_title(track);
        let title_key = title.to_lowercase();
        if seen_albums.insert(title_key) {
            let tracks_to_upload: Vec<ScannedTrack> = if upload_full_albums {
                all_library_tracks
                    .iter()
                    .filter(|t| get_album_title(t).eq_ignore_ascii_case(&title))
                    .cloned()
                    .collect()
            } else {
                playlist_tracks
                    .iter()
                    .filter(|t| get_album_title(t).eq_ignore_ascii_case(&title))
                    .cloned()
                    .collect()
            };

            if !tracks_to_upload.is_empty() {
                album_groups.push((title, tracks_to_upload));
            }
        }
    }

    album_groups
}

pub fn group_local_tracks_by_album(
    tracks: &[ScannedTrack],
) -> Vec<(String, Vec<ScannedTrack>)> {
    let get_album_title = |t: &ScannedTrack| -> String {
        t.mapped_fields
            .get("album")
            .and_then(|a| a.first())
            .map(|s| s.as_str())
            .unwrap_or(&t.file_name)
            .trim()
            .to_string()
    };

    let mut album_groups: Vec<(String, Vec<ScannedTrack>)> = Vec::new();
    let mut album_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for track in tracks {
        if track.path.starts_with("remote://") {
            continue;
        }
        let title = get_album_title(track);
        let key = title.to_lowercase();
        if let Some(&idx) = album_map.get(&key) {
            album_groups[idx].1.push(track.clone());
        } else {
            let idx = album_groups.len();
            album_map.insert(key, idx);
            album_groups.push((title, vec![track.clone()]));
        }
    }

    album_groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_m3u_playlist() {
        let content = r#"
#EXTM3U
#EXTINF:123,Bach - Toccata and Fugue
C:\Music\Bach\toccata.flac
#EXTINF:456,Mozart - Symphony 40
../Mozart/symphony40.mp3

# Comment line
Beethoven/symphony9.ogg
"#;

        let tracks = parse_playlist_content(content, "m3u");
        assert_eq!(tracks, vec![
            ("C:\\Music\\Bach\\toccata.flac".to_string(), Some("Bach - Toccata and Fugue".to_string())),
            ("../Mozart/symphony40.mp3".to_string(), Some("Mozart - Symphony 40".to_string())),
            ("Beethoven/symphony9.ogg".to_string(), None),
        ]);
    }

    #[test]
    fn test_parse_pls_playlist() {
        let content = r#"
[playlist]
File1=C:\Music\Bach\toccata.flac
Title1=Bach - Toccata and Fugue
Length1=123
file2=../Mozart/symphony40.mp3
title2=Mozart - Symphony 40
length2=456
FILE3=Beethoven/symphony9.ogg
NumberOfEntries=3
Version=2
"#;

        let tracks = parse_playlist_content(content, "pls");
        assert_eq!(tracks, vec![
            ("C:\\Music\\Bach\\toccata.flac".to_string(), Some("Bach - Toccata and Fugue".to_string())),
            ("../Mozart/symphony40.mp3".to_string(), Some("Mozart - Symphony 40".to_string())),
            ("Beethoven/symphony9.ogg".to_string(), None),
        ]);
    }

    #[test]
    fn test_clean_and_normalize_path() {
        let base_path = std::path::Path::new("C:\\Music\\Playlists");
        let rel_path = std::path::Path::new("..\\Bach\\toccata.flac");
        let resolved = base_path.join(rel_path);

        assert_eq!(resolved.to_string_lossy(), "C:\\Music\\Playlists\\..\\Bach\\toccata.flac");

        let normalized = normalize_for_comparison(&resolved);
        assert_eq!(normalized, "c:/music/bach/toccata.flac");

        let dot_path = std::path::Path::new("D:\\Music\\.\\song.mp3");
        assert_eq!(normalize_for_comparison(dot_path), "d:/music/song.mp3");
    }

    #[test]
    fn test_decode_windows1252() {
        let cp1252_bytes = b"V\xedkingur \xd3lafsson";
        #[cfg(target_os = "windows")]
        {
            let decoded = decode_with_codepage(cp1252_bytes, 1252).unwrap();
            assert_eq!(decoded, "Víkingur Ólafsson");
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert!(decode_with_codepage(cp1252_bytes, 1252).is_err());
        }
    }

    #[test]
    fn test_group_playlist_tracks_by_album_playlist_only() {
        let mut t1 = ScannedTrack::default();
        t1.id = "t1".into();
        t1.file_name = "01.flac".into();
        t1.mapped_fields.insert("album".into(), vec!["Album A".into()]);

        let mut t2 = ScannedTrack::default();
        t2.id = "t2".into();
        t2.file_name = "02.flac".into();
        t2.mapped_fields.insert("album".into(), vec!["Album B".into()]);

        let mut t3 = ScannedTrack::default();
        t3.id = "t3".into();
        t3.file_name = "03.flac".into();
        t3.mapped_fields.insert("album".into(), vec!["Album A".into()]);

        let playlist_tracks = vec![t1.clone(), t2.clone(), t3.clone()];
        let all_library_tracks = vec![t1.clone(), t2.clone(), t3.clone()];

        let groups = group_playlist_tracks_by_album(&playlist_tracks, &all_library_tracks, false);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "Album A");
        assert_eq!(groups[0].1.len(), 2);
        assert_eq!(groups[0].1[0].id, "t1");
        assert_eq!(groups[0].1[1].id, "t3");

        assert_eq!(groups[1].0, "Album B");
        assert_eq!(groups[1].1.len(), 1);
        assert_eq!(groups[1].1[0].id, "t2");
    }

    #[test]
    fn test_group_playlist_tracks_by_album_full_albums() {
        let mut t1 = ScannedTrack::default();
        t1.id = "t1".into();
        t1.file_name = "01.flac".into();
        t1.mapped_fields.insert("album".into(), vec!["Album A".into()]);

        let mut t2 = ScannedTrack::default();
        t2.id = "t2".into();
        t2.file_name = "02.flac".into();
        t2.mapped_fields.insert("album".into(), vec!["Album A".into()]);

        let mut t3 = ScannedTrack::default();
        t3.id = "t3".into();
        t3.file_name = "03.flac".into();
        t3.mapped_fields.insert("album".into(), vec!["Album A".into()]);

        // Playlist only has t1
        let playlist_tracks = vec![t1.clone()];
        let all_library_tracks = vec![t1.clone(), t2.clone(), t3.clone()];

        let groups = group_playlist_tracks_by_album(&playlist_tracks, &all_library_tracks, true);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "Album A");
        assert_eq!(groups[0].1.len(), 3);
    }

    #[test]
    fn test_playlist_track_matcher_diacritics() {
        let mut track = ScannedTrack::default();
        track.id = "dvorak_track_1".into();
        track.path = "G:\\Rosettes\\Penguin Rosettes - 1\\Domus - Dvořák Piano Quartets Nos. 1 & 2 (1988) [16B-44.1kHz]\\01. Piano Quartet No. 1 in D Major, Op. 23, B. 53 I. Allegro moderato.flac".into();
        track.file_name = "01. Piano Quartet No. 1 in D Major, Op. 23, B. 53 I. Allegro moderato.flac".into();

        let library_tracks = vec![track];
        let matcher = PlaylistTrackMatcher::new(&library_tracks);

        // In the playlist, Dvorák is written with ASCII 'r' + 'á' instead of Czech 'ř'
        let playlist_path = std::path::Path::new("G:\\Rosettes\\Penguin Rosettes - 1\\Domus - Dvorák Piano Quartets Nos. 1 & 2 (1988) [16B-44.1kHz]\\01. Piano Quartet No. 1 in D Major, Op. 23, B. 53 I. Allegro moderato.flac");
        let matched = matcher.find_match(playlist_path);
        assert_eq!(matched.as_deref(), Some("dvorak_track_1"));

        // Also test plain ASCII "Dvorak"
        let ascii_path = std::path::Path::new("G:\\Rosettes\\Penguin Rosettes - 1\\Domus - Dvorak Piano Quartets Nos. 1 & 2 (1988) [16B-44.1kHz]\\01. Piano Quartet No. 1 in D Major, Op. 23, B. 53 I. Allegro moderato.flac");
        let matched_ascii = matcher.find_match(ascii_path);
        assert_eq!(matched_ascii.as_deref(), Some("dvorak_track_1"));
    }

    #[test]
    fn test_playlist_track_matcher_parent_folder_or_drive_letter_change() {
        let mut track = ScannedTrack::default();
        track.id = "bach_track".into();
        track.path = "D:\\Classical\\Bach\\Goldberg\\01. Aria.flac".into();
        track.file_name = "01. Aria.flac".into();

        let mut other_track = ScannedTrack::default();
        other_track.id = "mozart_track".into();
        other_track.path = "D:\\Classical\\Mozart\\01. Aria.flac".into();
        other_track.file_name = "01. Aria.flac".into();

        let library_tracks = vec![track, other_track];
        let matcher = PlaylistTrackMatcher::new(&library_tracks);

        // Playlist path has different drive letter E: but same parent folder Goldberg
        let playlist_path = std::path::Path::new("E:\\OldMusic\\Goldberg\\01. Aria.flac");
        let matched = matcher.find_match(playlist_path);
        assert_eq!(matched.as_deref(), Some("bach_track"));
    }

    #[test]
    fn test_playlist_track_matcher_greek_beta_homoglyph() {
        let mut track = ScannedTrack::default();
        track.id = "petibon_track_29".into();
        // On disk/in DB: uses Greek small letter beta '\u{03b2}' in "flieβen"
        track.path = "G:\\Mozart 50\\Patricia Petibon - Mozart Die Entführung aus dem Serail, K. 384 (1999) [16B-44.1kHz]\\29. Die Entführung aus dem Serail, K. 384, Act 2 Arie. Wenn der Freude Tränen flie\u{03b2}en (Belmonte).flac".into();
        track.file_name = "29. Die Entführung aus dem Serail, K. 384, Act 2 Arie. Wenn der Freude Tränen flie\u{03b2}en (Belmonte).flac".into();

        let library_tracks = vec![track];
        let matcher = PlaylistTrackMatcher::new(&library_tracks);

        // In playlist: uses German sharp s 'ß' (U+00DF) in "fließen"
        let playlist_path = std::path::Path::new("G:\\Mozart 50\\Patricia Petibon - Mozart Die Entführung aus dem Serail, K. 384 (1999) [16B-44.1kHz]\\29. Die Entführung aus dem Serail, K. 384, Act 2 Arie. Wenn der Freude Tränen fließen (Belmonte).flac");
        let matched = matcher.find_match(playlist_path);
        assert_eq!(matched.as_deref(), Some("petibon_track_29"));
    }

    #[test]
    fn test_playlist_track_matcher_track_number_fallback() {
        let mut track = ScannedTrack::default();
        track.id = "aria_track_29".into();
        track.path = "C:\\Music\\Mozart\\Album\\29. Arie. Wenn der Freude Traenen fliessen.flac".into();
        track.file_name = "29. Arie. Wenn der Freude Traenen fliessen.flac".into();

        let library_tracks = vec![track];
        let matcher = PlaylistTrackMatcher::new(&library_tracks);

        // Playlist has quotation marks and different spelling in filename, but same parent album and track number 29
        let playlist_path = std::path::Path::new("C:\\Music\\Mozart\\Album\\29. Arie. \"Wenn der Freude\".flac");
        let matched = matcher.find_match(playlist_path);
        assert_eq!(matched.as_deref(), Some("aria_track_29"));
    }

    #[test]
    fn test_group_local_tracks_by_album_ignores_remote_tracks() {
        let mut t1 = ScannedTrack::default();
        t1.id = "t1".into();
        t1.path = "D:\\Music\\Beethoven\\Symphony5\\01.flac".into();
        t1.mapped_fields.insert("album".into(), vec!["Symphony No. 5".into()]);

        let mut t2 = ScannedTrack::default();
        t2.id = "t2".into();
        t2.path = "D:\\Music\\Beethoven\\Symphony5\\02.flac".into();
        t2.mapped_fields.insert("album".into(), vec!["Symphony No. 5".into()]);

        let mut t3_remote = ScannedTrack::default();
        t3_remote.id = "t3".into();
        t3_remote.path = "remote://target_1/Mozart/Requiem/01.flac".into();
        t3_remote.mapped_fields.insert("album".into(), vec!["Requiem".into()]);

        let mut t4 = ScannedTrack::default();
        t4.id = "t4".into();
        t4.path = "D:\\Music\\Bach\\Goldberg\\01.flac".into();
        t4.mapped_fields.insert("album".into(), vec!["Goldberg Variations".into()]);

        let groups = group_local_tracks_by_album(&[t1, t2, t3_remote, t4]);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, "Symphony No. 5");
        assert_eq!(groups[0].1.len(), 2);
        assert_eq!(groups[1].0, "Goldberg Variations");
        assert_eq!(groups[1].1.len(), 1);
    }
}
