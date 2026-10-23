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
    CachePrefetchProgress(CachePrefetchEvent),
    GoogleAuthCompleted(GoogleAuthCompletedEvent),
    SyncProgress(RemoteSyncProgressEvent),
    SyncCompleted(RemoteSyncCompletedEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoogleAuthCompletedEvent {
    pub success: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CachePrefetchEvent {
    pub track_id: String,
    pub title: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub is_completed: bool,
    pub error_message: Option<String>,
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
pub struct UploadFailedItem {
    pub item_id: Option<String>,
    pub title: String,
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UploadCompletedEvent {
    pub target_id: String,
    pub album_id: String,
    pub album_title: String,
    pub success: bool,
    pub error_message: Option<String>,
    #[serde(default)]
    pub failed_items: Vec<UploadFailedItem>,
    #[serde(default)]
    pub new_tracks_count: usize,
    #[serde(default)]
    pub already_synced_tracks_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSyncProgressEvent {
    pub target_id: String,
    pub target_name: String,
    pub current_album: String,
    pub completed_albums: usize,
    pub total_albums: usize,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSyncCompletedEvent {
    pub target_id: String,
    pub target_name: String,
    pub new_albums_count: usize,
    pub new_tracks_count: usize,
    #[serde(default)]
    pub new_playlists_count: usize,
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
