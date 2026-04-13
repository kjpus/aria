use std::sync::Arc;

use aria_domain::{PlaybackPreferences, SettingsSnapshot, ThemePreference, TrackTableSettings};
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct SettingsStore {
    state: Arc<RwLock<SettingsSnapshot>>,
}

impl SettingsStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_snapshot(snapshot: SettingsSnapshot) -> Self {
        Self {
            state: Arc::new(RwLock::new(snapshot)),
        }
    }

    pub async fn snapshot(&self) -> SettingsSnapshot {
        self.state.read().await.clone()
    }

    pub async fn update_theme(&self, theme: ThemePreference) -> SettingsSnapshot {
        let mut state = self.state.write().await;
        state.theme = theme;
        state.clone()
    }

    pub async fn update_track_table(&self, track_table: TrackTableSettings) -> SettingsSnapshot {
        let mut state = self.state.write().await;
        state.track_table = track_table;
        state.clone()
    }

    pub async fn update_album_track_table(
        &self,
        album_track_table: TrackTableSettings,
    ) -> SettingsSnapshot {
        let mut state = self.state.write().await;
        state.album_track_table = album_track_table;
        state.clone()
    }

    pub async fn update_playlist_track_table(
        &self,
        playlist_track_table: TrackTableSettings,
    ) -> SettingsSnapshot {
        let mut state = self.state.write().await;
        state.playlist_track_table = playlist_track_table;
        state.clone()
    }

    pub async fn update_playback(&self, playback: PlaybackPreferences) -> SettingsSnapshot {
        let mut state = self.state.write().await;
        state.playback = playback;
        state.clone()
    }
}
