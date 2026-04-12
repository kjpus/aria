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
    pub catalog_rules: Vec<CatalogPatternRule>,
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
pub struct CatalogPatternRule {
    pub label: String,
    pub pattern: String,
    #[serde(default)]
    pub composers: Vec<String>,
    #[serde(default = "default_catalog_rule_source_tags")]
    pub source_tags: Vec<String>,
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

pub fn default_catalog_rules() -> Vec<CatalogPatternRule> {
    vec![
        CatalogPatternRule {
            label: "Opus".into(),
            pattern: r"(?i)\b(?:Op\.?|Opus)\s*\d+[A-Za-z]?(?:\s*No\.?\s*\d+)?\b".into(),
            composers: Vec::new(),
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "BWV".into(),
            pattern: r"(?i)\bBWV\s*\d+[A-Za-z]?\b".into(),
            composers: vec!["Johann Sebastian Bach".into(), "Bach".into()],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "WAB".into(),
            pattern: r"(?i)\bWAB\s*\d+[A-Za-z]?\b".into(),
            composers: vec!["Anton Bruckner".into(), "Bruckner".into()],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "K".into(),
            pattern: r"(?i)\bK\.?\s*\d+[A-Za-z]?\b".into(),
            composers: vec!["Wolfgang Amadeus Mozart".into(), "Mozart".into()],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "KV".into(),
            pattern: r"(?i)\bKV\s*\d+[A-Za-z]?\b".into(),
            composers: vec!["Wolfgang Amadeus Mozart".into(), "Mozart".into()],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "D".into(),
            pattern: r"(?i)\bD\.?\s*\d+[A-Za-z]?\b".into(),
            composers: vec!["Franz Schubert".into(), "Schubert".into()],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "RV".into(),
            pattern: r"(?i)\bRV\s*\d+[A-Za-z]?\b".into(),
            composers: vec!["Antonio Vivaldi".into(), "Vivaldi".into()],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "HWV".into(),
            pattern: r"(?i)\bHWV\s*\d+[A-Za-z]?\b".into(),
            composers: vec![
                "George Frideric Handel".into(),
                "Georg Friedrich Handel".into(),
                "Handel".into(),
            ],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "TWV".into(),
            pattern: r"(?i)\bTWV\s*\d+:\d+\b".into(),
            composers: vec!["Georg Philipp Telemann".into(), "Telemann".into()],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "BuxWV".into(),
            pattern: r"(?i)\bBuxWV\s*\d+[A-Za-z]?\b".into(),
            composers: vec!["Dieterich Buxtehude".into(), "Buxtehude".into()],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "Hob.".into(),
            pattern: r"(?i)\bHob\.?\s*[IVXLC]+[:. ]\s*\d+\b".into(),
            composers: vec![
                "Joseph Haydn".into(),
                "Franz Joseph Haydn".into(),
                "Haydn".into(),
            ],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "S.".into(),
            pattern: r"(?i)\bS\.?\s*\d+[A-Za-z]?\b".into(),
            composers: vec!["Franz Liszt".into(), "Liszt".into()],
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
        CatalogPatternRule {
            label: "WoO".into(),
            pattern: r"(?i)\bWoO\s*\d+[A-Za-z]?\b".into(),
            composers: Vec::new(),
            source_tags: default_catalog_rule_source_tags(),
            enabled: true,
        },
    ]
}

fn default_catalog_rule_source_tags() -> Vec<String> {
    vec!["TITLE".into(), "WORK".into(), "ALBUM".into()]
}

fn default_catalog_rule_enabled() -> bool {
    true
}
