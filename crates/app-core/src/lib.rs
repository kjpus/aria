use aria_domain::{
    AppBootstrap, AppEvent, CatalogRule, FieldExportRequest, LibraryEvent, LibraryFieldMapping,
    LibrarySnapshot, OutputDeviceSnapshot, PlayTrackRequest, PlaybackEvent, PlaybackPreferences,
    PlaybackSessionSnapshot, PlaybackSnapshot, PlaybackStatus, PlaylistEvent, PlaylistImportPreview,
    PlaylistSnapshot, PreviewTrack, RemoteBackendType, RemoteEvent, RemoteSyncedItem, RemoteTarget,
    ScannedTrack, SettingsSnapshot, ThemePreference, TrackTableSettings, TrackTagEditRequest,
    UploadCompletedEvent,
};
use aria_library::{LibraryError, LibraryService};
use aria_playback::{PlaybackError, PlaybackService};
use aria_playlists::{PlaylistError, PlaylistService};
use aria_remote_storage::{
    create_backend_for_target, upload_album, GoogleAuthConfig, GoogleDriveStoredConfig,
    PathUpdater, PendingAuthFlow, PlaybackPrefetcher, PrefetchCandidate, RemoteCacheManager,
    RemoteCacheStatus, RemoteStorageError, StorageStatus, UploadOptions,
};
use aria_storage::{AppDatabase, SettingsStore, StorageError};
use std::collections::BTreeMap;
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

#[derive(Clone)]
pub struct AppCore {
    events: broadcast::Sender<AppEvent>,
    library: LibraryService,
    playback: PlaybackService,
    playlists: PlaylistService,
    settings: SettingsStore,
    database: AppDatabase,
    pending_gdrive_auth: Arc<Mutex<Option<PendingAuthFlow>>>,
    cache_manager: Arc<RemoteCacheManager>,
    prefetcher: Arc<PlaybackPrefetcher>,
}

impl AppCore {
    pub fn new() -> Result<Self, AppCoreError> {
        let database = AppDatabase::new_default()?;
        let persisted = database.load_state()?;
        let (events, _) = broadcast::channel(64);
        let playback = PlaybackService::with_session(
            persisted.settings.playback.clone(),
            persisted.playback,
        );

        let cache_dir = RemoteCacheManager::default_cache_dir().unwrap_or_else(|_| {
            std::env::temp_dir().join("aria_remote_cache")
        });
        let cache_manager = Arc::new(RemoteCacheManager::new(cache_dir, 3));

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
            pending_gdrive_auth: Arc::new(Mutex::new(None)),
            cache_manager,
            prefetcher,
        };

        core.spawn_persistence_worker(database);
        core.spawn_playback_publisher();
        core.spawn_output_device_monitor();
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
        let track_lookup: std::collections::HashMap<String, String> = library
            .tracks
            .iter()
            .map(|track| {
                let norm_path = normalize_for_comparison(std::path::Path::new(&track.path));
                (norm_path, track.id.clone())
            })
            .collect();

        let mut preview_tracks = Vec::new();
        for (raw_path, raw_title) in raw_tracks {
            let p = std::path::Path::new(&raw_path);
            let resolved = if p.is_absolute() {
                p.to_path_buf()
            } else {
                parent_dir.join(p)
            };
            let norm = normalize_for_comparison(&resolved);
            let track_id = track_lookup.get(&norm).cloned();

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
        if let Some(idx) = session.current_queue_index {
            if let Some(mut req) = session.queue.get(idx).cloned() {
                let window: Vec<String> = session
                    .queue
                    .iter()
                    .skip(idx)
                    .take(3)
                    .map(|r| r.queue_item.id.clone())
                    .collect();
                let _ = self.resolve_remote_track_if_needed(&mut req, &window).await;
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
        let _ = self
            .resolve_remote_track_if_needed(&mut request, &active_window)
            .await;
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
            let _ = self
                .resolve_remote_track_if_needed(&mut requests[0], &active_window)
                .await;
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
                    let _ = self
                        .resolve_remote_track_if_needed(&mut prev_req, &window)
                        .await;
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
                let _ = self
                    .resolve_remote_track_if_needed(&mut next_req, &window)
                    .await;
            }
        }
        let snapshot = self.playback.next_track().await?;
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
            request.path = cached_path.to_string_lossy().to_string();
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
        let targets = self.database.load_remote_targets()?;
        let target = targets
            .into_iter()
            .find(|t| t.id == target_id)
            .ok_or_else(|| AppCoreError::Remote(format!("Target '{target_id}' not found")))?;
        let backend = create_backend_for_target(&target)?;

        let local_path = self
            .cache_manager
            .get_or_download_track(
                backend.as_ref(),
                &rel_path,
                track_id,
                checksum.as_deref(),
                active_window_ids,
            )
            .await?;

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
        for i in 0..3 {
            if let Some(item) = session.queue.get(current_idx + i) {
                active_window_ids.push(item.queue_item.id.clone());
            }
        }

        let targets = match self.database.load_remote_targets() {
            Ok(t) => t,
            Err(_) => return,
        };

        let mut candidates = Vec::new();
        for i in 1..=2 {
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

                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

                        let snapshot = playback.snapshot().await;
                        if snapshot != last_snapshot {
                            let track_changed = snapshot.current_track != last_snapshot.current_track;
                            last_snapshot = snapshot.clone();
                            let _ = events
                                .send(AppEvent::Playback(PlaybackEvent::SnapshotChanged(snapshot)));
                            if track_changed {
                                core.trigger_sliding_window_prefetch().await;
                            }
                        }
                    }
                });
            })
            .expect("failed to spawn aria-playback-publisher");
    }

    fn spawn_output_device_monitor(&self) {
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

    pub fn get_remote_targets(&self) -> Result<Vec<RemoteTarget>, AppCoreError> {
        Ok(self.database.load_remote_targets()?)
    }

    pub fn save_remote_target(&self, target: RemoteTarget) -> Result<Vec<RemoteTarget>, AppCoreError> {
        self.database.save_remote_target(&target)?;
        let targets = self.database.load_remote_targets()?;
        let _ = self.events.send(AppEvent::Remote(RemoteEvent::TargetsChanged(targets.clone())));
        Ok(targets)
    }

    pub fn delete_remote_target(&self, target_id: &str) -> Result<Vec<RemoteTarget>, AppCoreError> {
        self.database.delete_remote_target(target_id)?;
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
    ) -> Result<String, AppCoreError> {
        let auth_config = GoogleAuthConfig {
            client_id,
            client_secret,
        };
        let flow = PendingAuthFlow::start(auth_config).await?;
        let auth_url = flow.authorization_url.clone();
        let mut pending = self.pending_gdrive_auth.lock().await;
        *pending = Some(flow);
        Ok(auth_url)
    }

    pub async fn complete_gdrive_auth_flow(
        &self,
        target_name: String,
        storage_limit_bytes: Option<u64>,
    ) -> Result<RemoteTarget, AppCoreError> {
        let flow = {
            let mut pending = self.pending_gdrive_auth.lock().await;
            pending
                .take()
                .ok_or_else(|| AppCoreError::Remote("No pending Google Drive auth flow".into()))?
        };

        let tokens = flow.wait_for_tokens().await?;
        let stored_cfg = GoogleDriveStoredConfig {
            client_id: "from_oauth".to_string(),
            client_secret: None,
            access_token: Some(tokens.access_token),
            refresh_token: tokens.refresh_token,
            expires_at: Some(tokens.expires_at),
            root_folder_name: "Aria".to_string(),
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
                Ok(synced_items) => {
                    for item in &synced_items {
                        let _ = core.database.save_remote_synced_item(item);
                    }
                    let album_id = synced_items
                        .iter()
                        .find(|i| i.item_type == "album")
                        .map(|i| i.item_id.clone())
                        .unwrap_or_default();
                    let _ = core.events.send(AppEvent::Remote(RemoteEvent::UploadCompleted(
                        UploadCompletedEvent {
                            target_id: target.id.clone(),
                            album_id,
                            album_title: album_title.clone(),
                            success: true,
                            error_message: None,
                        },
                    )));
                }
                Err(e) => {
                    let _ = core.events.send(AppEvent::Remote(RemoteEvent::UploadCompleted(
                        UploadCompletedEvent {
                            target_id: target.id.clone(),
                            album_id: String::new(),
                            album_title: album_title.clone(),
                            success: false,
                            error_message: Some(e.to_string()),
                        },
                    )));
                }
            }
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
}
