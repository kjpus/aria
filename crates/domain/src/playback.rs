use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackStatus {
    Stopped,
    Paused,
    Playing,
    Buffering,
}

impl Default for PlaybackStatus {
    fn default() -> Self {
        Self::Stopped
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlayTrackRequest {
    pub path: String,
    pub queue_item: QueueItem,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OutputDeviceSnapshot {
    pub id: String,
    pub name: String,
    pub backend: String,
    pub exclusive_capable: bool,
    #[serde(default)]
    pub is_default: bool,
}

impl Default for OutputDeviceSnapshot {
    fn default() -> Self {
        Self {
            id: "system-default".into(),
            name: "System Default".into(),
            backend: "placeholder".into(),
            exclusive_capable: false,
            is_default: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSnapshot {
    pub status: PlaybackStatus,
    pub current_track: Option<QueueItem>,
    #[serde(default)]
    pub queue: Vec<QueueItem>,
    #[serde(default)]
    pub current_queue_index: Option<usize>,
    pub queue_depth: usize,
    pub position_ms: u64,
    pub output_device: OutputDeviceSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackSessionSnapshot {
    #[serde(default)]
    pub queue: Vec<PlayTrackRequest>,
    #[serde(default)]
    pub ordered_queue: Vec<PlayTrackRequest>,
    #[serde(default)]
    pub current_queue_index: Option<usize>,
}
