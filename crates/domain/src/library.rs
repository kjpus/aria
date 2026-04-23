use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRoot {
    pub path: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibrarySnapshot {
    pub roots: Vec<LibraryRoot>,
    pub is_scanning: bool,
    pub indexed_files: u64,
    pub last_scan_at: Option<String>,
    pub field_mappings: Vec<LibraryFieldMapping>,
    #[serde(default = "default_catalog_rules")]
    pub catalog_rules: Vec<CatalogRule>,
    pub tag_inventory: Vec<TagInventoryEntry>,
    pub tracks: Vec<ScannedTrack>,
}

impl Default for LibrarySnapshot {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            is_scanning: false,
            indexed_files: 0,
            last_scan_at: None,
            field_mappings: default_field_mappings(),
            catalog_rules: default_catalog_rules(),
            tag_inventory: Vec::new(),
            tracks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub phase: String,
    pub processed_files: u64,
    pub discovered_files: u64,
    pub failed_files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFieldMapping {
    pub key: String,
    pub label: String,
    pub tag_priorities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FieldExportRequest {
    pub track_paths: Vec<String>,
    pub field_key: String,
    pub tag_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackTagEditUpdate {
    pub tag_name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackTagEditRequest {
    pub track_paths: Vec<String>,
    pub updates: Vec<TrackTagEditUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogRule {
    pub label: String,
    #[serde(default)]
    pub composers: Vec<String>,
    #[serde(default = "default_catalog_rule_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TagInventoryEntry {
    pub tag: String,
    pub occurrences: u64,
    pub example_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AudioPropertiesSnapshot {
    pub format: String,
    pub duration_ms: u64,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScannedTrack {
    pub id: String,
    pub path: String,
    pub file_name: String,
    pub album_art_path: Option<String>,
    pub audio: AudioPropertiesSnapshot,
    pub raw_tags: BTreeMap<String, Vec<String>>,
    pub mapped_fields: BTreeMap<String, Vec<String>>,
}

pub fn default_field_mappings() -> Vec<LibraryFieldMapping> {
    vec![
        LibraryFieldMapping {
            key: "album".into(),
            label: "Album".into(),
            tag_priorities: vec!["ALBUM".into()],
        },
        LibraryFieldMapping {
            key: "title".into(),
            label: "Title".into(),
            tag_priorities: vec!["TITLE".into()],
        },
        LibraryFieldMapping {
            key: "catalog".into(),
            label: "Catalog".into(),
            tag_priorities: vec!["CATALOGNUMBER".into(), "CATALOG".into()],
        },
        LibraryFieldMapping {
            key: "composer".into(),
            label: "Composer".into(),
            tag_priorities: vec!["COMPOSER".into()],
        },
        LibraryFieldMapping {
            key: "genre".into(),
            label: "Genre".into(),
            tag_priorities: vec!["GENRE".into()],
        },
        LibraryFieldMapping {
            key: "conductor".into(),
            label: "Conductor".into(),
            tag_priorities: vec!["CONDUCTOR".into()],
        },
        LibraryFieldMapping {
            key: "ensemble".into(),
            label: "Ensemble".into(),
            tag_priorities: vec!["ENSEMBLE".into(), "ORCHESTRA".into(), "ALBUMARTIST".into()],
        },
        LibraryFieldMapping {
            key: "soloist".into(),
            label: "Soloist".into(),
            tag_priorities: vec!["PERFORMER".into(), "ARTIST".into(), "ALBUMARTIST".into()],
        },
        LibraryFieldMapping {
            key: "year".into(),
            label: "Year".into(),
            tag_priorities: vec!["DATE".into(), "YEAR".into()],
        },
        LibraryFieldMapping {
            key: "disk_number".into(),
            label: "Disk Number".into(),
            tag_priorities: vec!["DISCNUMBER".into()],
        },
        LibraryFieldMapping {
            key: "track_number".into(),
            label: "Track Number".into(),
            tag_priorities: vec!["TRACKNUMBER".into()],
        },
    ]
}

pub fn default_catalog_rules() -> Vec<CatalogRule> {
    vec![
        catalog_rule("BWV", &["Johann Sebastian Bach", "Bach"]),
        catalog_rule("WAB", &["Anton Bruckner", "Bruckner"]),
        catalog_rule("K", &["Wolfgang Amadeus Mozart", "Mozart"]),
        catalog_rule("KV", &["Wolfgang Amadeus Mozart", "Mozart"]),
        catalog_rule("D", &["Franz Schubert", "Schubert"]),
        catalog_rule("RV", &["Antonio Vivaldi", "Vivaldi"]),
        catalog_rule(
            "HWV",
            &[
                "George Frideric Handel",
                "Georg Friedrich Handel",
                "Handel",
            ],
        ),
        catalog_rule("TWV", &["Georg Philipp Telemann", "Telemann"]),
        catalog_rule("BuxWV", &["Dieterich Buxtehude", "Buxtehude"]),
        catalog_rule("Hob.", &["Joseph Haydn", "Franz Joseph Haydn", "Haydn"]),
        catalog_rule("S.", &["Franz Liszt", "Liszt"]),
        catalog_rule("WoO", &[]),
        catalog_rule("Op", &[]),
    ]
}

fn catalog_rule(label: &str, composers: &[&str]) -> CatalogRule {
    CatalogRule {
        label: label.into(),
        composers: composers.iter().map(|composer| (*composer).into()).collect(),
        enabled: true,
    }
}

fn default_catalog_rule_enabled() -> bool {
    true
}
