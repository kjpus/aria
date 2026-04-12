use serde::{Deserialize, Serialize};

use crate::{LibrarySnapshot, PlaybackSnapshot, PlaylistSnapshot, ScanProgress, SettingsSnapshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum LibraryEvent {
    SnapshotChanged(LibrarySnapshot),
    ScanProgress(ScanProgress),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum PlaybackEvent {
    SnapshotChanged(PlaybackSnapshot),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum PlaylistEvent {
    SnapshotChanged(PlaylistSnapshot),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "topic", content = "payload", rename_all = "snake_case")]
pub enum AppEvent {
    Library(LibraryEvent),
    Playback(PlaybackEvent),
    Playlists(PlaylistEvent),
    Settings(SettingsSnapshot),
}
