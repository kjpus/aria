pub mod events;
pub mod library;
pub mod playback;
pub mod playlist;
pub mod settings;

pub use events::{AppEvent, LibraryEvent, PlaybackEvent, PlaylistEvent};
pub use library::{
    default_catalog_rules, default_field_mappings, AudioPropertiesSnapshot, CatalogRule,
    FieldExportRequest, LibraryFieldMapping, LibraryRoot, LibrarySnapshot, ScanProgress,
    ScannedTrack, TagInventoryEntry,
};
pub use playback::{
    OutputDeviceSnapshot, PlayTrackRequest, PlaybackSessionSnapshot, PlaybackSnapshot,
    PlaybackStatus, QueueItem,
};
pub use playlist::{Playlist, PlaylistSnapshot};
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
}
