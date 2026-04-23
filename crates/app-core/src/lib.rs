use aria_domain::{
    AppBootstrap, AppEvent, CatalogRule, FieldExportRequest, LibraryEvent, LibraryFieldMapping,
    LibrarySnapshot, OutputDeviceSnapshot, PlayTrackRequest, PlaybackEvent, PlaybackPreferences,
    PlaylistEvent, PlaylistSnapshot, PlaybackSessionSnapshot, PlaybackSnapshot, SettingsSnapshot,
    ThemePreference, TrackTableSettings,
};
use aria_library::{LibraryError, LibraryService};
use aria_playback::{PlaybackError, PlaybackService};
use aria_playlists::{PlaylistError, PlaylistService};
use aria_storage::{AppDatabase, SettingsStore, StorageError};
use std::collections::BTreeMap;
use thiserror::Error;
use tokio::sync::broadcast;

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
}

#[derive(Clone)]
pub struct AppCore {
    events: broadcast::Sender<AppEvent>,
    library: LibraryService,
    playback: PlaybackService,
    playlists: PlaylistService,
    settings: SettingsStore,
}

impl AppCore {
    pub fn new() -> Result<Self, AppCoreError> {
        let database = AppDatabase::new_default()?;
        let persisted = database.load_state()?;
        let (events, _) = broadcast::channel(64);
        let core = Self {
            events,
            library: LibraryService::with_snapshot(persisted.library),
            playback: PlaybackService::with_session(
                persisted.settings.playback.clone(),
                persisted.playback,
            ),
            playlists: PlaylistService::with_snapshot(persisted.playlists),
            settings: SettingsStore::with_snapshot(persisted.settings),
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

    pub async fn play(&self) -> Result<PlaybackSnapshot, AppCoreError> {
        let snapshot = self.playback.play().await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn play_track(
        &self,
        request: PlayTrackRequest,
    ) -> Result<PlaybackSnapshot, AppCoreError> {
        let snapshot = self.playback.play_track(request).await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn add_to_queue(&self, requests: Vec<PlayTrackRequest>) -> PlaybackSnapshot {
        let snapshot = self.playback.add_to_queue(requests).await;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        snapshot
    }

    pub async fn replace_queue(
        &self,
        requests: Vec<PlayTrackRequest>,
        start_playing: bool,
    ) -> Result<PlaybackSnapshot, AppCoreError> {
        let snapshot = self.playback.replace_queue(requests, start_playing).await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn previous_track(&self) -> Result<PlaybackSnapshot, AppCoreError> {
        let snapshot = self.playback.previous_track().await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
    }

    pub async fn next_track(&self) -> Result<PlaybackSnapshot, AppCoreError> {
        let snapshot = self.playback.next_track().await?;
        self.emit(AppEvent::Playback(PlaybackEvent::SnapshotChanged(
            snapshot.clone(),
        )));
        Ok(snapshot)
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
                            last_snapshot = snapshot.clone();
                            let _ = events
                                .send(AppEvent::Playback(PlaybackEvent::SnapshotChanged(snapshot)));
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
