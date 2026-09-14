pub mod events;
pub mod library;
pub mod playback;
pub mod playlist;
pub mod remote;
pub mod settings;

pub use events::{
    AppEvent, LibraryEvent, PlaybackEvent, PlaylistEvent, RemoteEvent, UploadCompletedEvent,
    UploadProgressEvent,
};
pub use remote::{
    AlbumManifest, ManifestTrack, MasterAlbumSummary, MasterLibraryIndex, RemoteBackendType,
    RemoteSourceLocation, RemoteSyncedItem, RemoteTarget, CURRENT_MANIFEST_VERSION,
};
pub use library::{
    canonical_field_mapping_format, default_catalog_rules, default_field_mappings,
    AudioPropertiesSnapshot, CatalogRule, FieldExportRequest, LibraryFieldMapping, LibraryRoot,
    LibrarySnapshot, ScanProgress, ScannedTrack, TagInventoryEntry, TrackTagEditRequest,
    TrackTagEditUpdate,
};
pub use playback::{
    OutputDeviceSnapshot, PlayTrackRequest, PlaybackSessionSnapshot, PlaybackSnapshot,
    PlaybackStatus, QueueItem,
};
pub use playlist::{Playlist, PlaylistImportPreview, PlaylistSnapshot, PreviewTrack};
pub use settings::{
    default_album_track_table_settings, default_playlist_track_table_settings,
    default_track_table_settings, PlaybackPreferences, SettingsSnapshot, ThemePreference,
    TrackSortCriterion, TrackSortDirection, TrackTableSettings,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrap {
    pub library: LibrarySnapshot,
    pub playback: PlaybackSnapshot,
    pub playlists: PlaylistSnapshot,
    pub settings: SettingsSnapshot,
    #[serde(default)]
    pub remote_targets: Vec<RemoteTarget>,
}
