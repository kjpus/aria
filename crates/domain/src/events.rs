use serde::{Deserialize, Serialize};

use crate::{
    LibrarySnapshot, OutputDeviceSnapshot, PlaybackSnapshot, PlaylistSnapshot, ScanProgress,
    SettingsSnapshot,
};

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
    OutputDevicesChanged(Vec<OutputDeviceSnapshot>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum PlaylistEvent {
    SnapshotChanged(PlaylistSnapshot),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum RemoteEvent {
    TargetsChanged(Vec<crate::RemoteTarget>),
    UploadProgress(UploadProgressEvent),
    UploadCompleted(UploadCompletedEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UploadProgressEvent {
    pub target_id: String,
    pub album_id: String,
    pub album_title: String,
    pub current_file: String,
    pub current_file_bytes: u64,
    pub current_file_total_bytes: u64,
    pub total_files: usize,
    pub completed_files: usize,
    pub total_bytes: u64,
    pub completed_bytes: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UploadCompletedEvent {
    pub target_id: String,
    pub album_id: String,
    pub album_title: String,
    pub success: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "topic", content = "payload", rename_all = "snake_case")]
pub enum AppEvent {
    Library(LibraryEvent),
    Playback(PlaybackEvent),
    Playlists(PlaylistEvent),
    Remote(RemoteEvent),
    Settings(SettingsSnapshot),
}
