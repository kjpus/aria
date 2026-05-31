use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub collage_seed: u32,
    #[serde(default)]
    pub track_ids: Vec<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistSnapshot {
    #[serde(default)]
    pub playlists: Vec<Playlist>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PreviewTrack {
    pub title: String,
    pub path: String,
    pub track_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistImportPreview {
    pub file_path: String,
    pub name: String,
    pub codepage: u32,
    pub system_default_codepage: u32,
    pub tracks: Vec<PreviewTrack>,
}
