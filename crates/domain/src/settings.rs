use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    System,
    Light,
    Dark,
}

impl Default for ThemePreference {
    fn default() -> Self {
        Self::System
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrackSortDirection {
    Asc,
    Desc,
}

impl Default for TrackSortDirection {
    fn default() -> Self {
        Self::Asc
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackSortCriterion {
    pub key: String,
    pub direction: TrackSortDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackTableSettings {
    pub visible_columns: Vec<String>,
    pub column_widths: BTreeMap<String, u32>,
    pub sort_key: String,
    pub sort_direction: TrackSortDirection,
    #[serde(default)]
    pub secondary_sort: Vec<TrackSortCriterion>,
}

impl Default for TrackTableSettings {
    fn default() -> Self {
        default_track_table_settings()
    }
}

pub fn default_track_table_settings() -> TrackTableSettings {
    TrackTableSettings {
        visible_columns: vec![
            "track_number".into(),
            "title".into(),
            "composer".into(),
            "conductor".into(),
            "album".into(),
            "year".into(),
            "format".into(),
        ],
        column_widths: default_track_column_widths(),
        sort_key: "album".into(),
        sort_direction: TrackSortDirection::Asc,
        secondary_sort: Vec::new(),
    }
}

pub fn default_album_track_table_settings() -> TrackTableSettings {
    TrackTableSettings {
        visible_columns: vec![
            "track_number".into(),
            "title".into(),
            "composer".into(),
            "conductor".into(),
            "ensemble".into(),
            "format".into(),
            "duration".into(),
        ],
        column_widths: default_track_column_widths(),
        sort_key: "track_number".into(),
        sort_direction: TrackSortDirection::Asc,
        secondary_sort: vec![TrackSortCriterion {
            key: "title".into(),
            direction: TrackSortDirection::Asc,
        }],
    }
}

fn default_track_column_widths() -> BTreeMap<String, u32> {
    BTreeMap::from([
        ("track_number".into(), 96),
        ("disk_number".into(), 96),
        ("year".into(), 110),
        ("format".into(), 110),
        ("duration".into(), 110),
        ("file_name".into(), 220),
        ("path".into(), 360),
        ("title".into(), 280),
        ("album".into(), 260),
        ("composer".into(), 220),
        ("conductor".into(), 220),
        ("ensemble".into(), 220),
        ("soloist".into(), 220),
    ])
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackPreferences {
    #[serde(default)]
    pub output_device_id: Option<String>,
    #[serde(default)]
    pub exclusive_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub theme: ThemePreference,
    pub accent_color: String,
    pub track_table: TrackTableSettings,
    #[serde(default = "default_album_track_table_settings")]
    pub album_track_table: TrackTableSettings,
    #[serde(default)]
    pub playback: PlaybackPreferences,
}

impl Default for SettingsSnapshot {
    fn default() -> Self {
        Self {
            theme: ThemePreference::System,
            accent_color: "#d6b16a".into(),
            track_table: default_track_table_settings(),
            album_track_table: default_album_track_table_settings(),
            playback: PlaybackPreferences::default(),
        }
    }
}
