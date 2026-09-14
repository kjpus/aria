use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

use crate::library::AudioPropertiesSnapshot;

pub const CURRENT_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteBackendType {
    GoogleDrive,
    Filesystem,
    WebDav,
    Smb,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTarget {
    pub id: String,
    pub name: String,
    pub backend_type: RemoteBackendType,
    pub storage_limit_bytes: Option<u64>,
    pub is_enabled: bool,
    #[serde(default)]
    pub config_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSourceLocation {
    pub target_id: String,
    pub backend_type: RemoteBackendType,
    pub remote_path_or_id: String,
    pub priority: u32,
    pub file_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSyncedItem {
    pub item_id: String,
    pub remote_target_id: String,
    pub item_type: String,
    pub remote_path: String,
    pub remote_file_id: Option<String>,
    pub size_bytes: u64,
    pub checksum: Option<String>,
    pub sync_status: String,
    pub last_synced_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestTrack {
    pub track_id: String,
    pub relative_file_name: String,
    pub file_name: String,
    pub file_size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_checksum_sha256: Option<String>,
    pub audio: AudioPropertiesSnapshot,
    pub raw_tags: BTreeMap<String, Vec<String>>,
    pub mapped_fields: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AlbumManifest {
    pub manifest_version: u32,
    pub album_id: String,
    pub title: String,
    #[serde(default)]
    pub composers: Vec<String>,
    #[serde(default)]
    pub conductors: Vec<String>,
    #[serde(default)]
    pub ensembles: Vec<String>,
    #[serde(default)]
    pub soloists: Vec<String>,
    #[serde(default)]
    pub catalog_numbers: Vec<String>,
    pub year: Option<String>,
    pub artwork_file_name: Option<String>,
    pub updated_at: String,
    pub tracks: Vec<ManifestTrack>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MasterAlbumSummary {
    pub album_id: String,
    pub title: String,
    #[serde(default)]
    pub composers: Vec<String>,
    #[serde(default)]
    pub conductors: Vec<String>,
    #[serde(default)]
    pub ensembles: Vec<String>,
    #[serde(default)]
    pub soloists: Vec<String>,
    #[serde(default)]
    pub catalog_numbers: Vec<String>,
    pub year: Option<String>,
    pub relative_folder: String,
    pub artwork_file_name: Option<String>,
    pub track_count: usize,
    pub total_duration_ms: u64,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MasterLibraryIndex {
    pub manifest_version: u32,
    pub updated_at: String,
    pub albums: Vec<MasterAlbumSummary>,
}
