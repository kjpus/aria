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
    #[serde(default)]
    pub format: String,
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
    let mut mappings = Vec::new();

    for format in ["DEFAULT", "FLAC", "MP3", "MP4", "AAC", "OGG", "OPUS", "WAV", "AIFF"] {
        mappings.extend(default_field_mappings_for_format(format));
    }

    mappings
}

pub fn canonical_field_mapping_format(format: &str) -> String {
    let normalized = format.trim().to_ascii_uppercase();

    match normalized.as_str() {
        "" => "DEFAULT".into(),
        "M4A" | "MP4" => "MP4".into(),
        "AIF" | "AIFF" => "AIFF".into(),
        other => other.to_string(),
    }
}

pub fn default_field_mappings_for_format(format: &str) -> Vec<LibraryFieldMapping> {
    let profile = default_mapping_profile(format);

    vec![
        default_field_mapping(format, "album", "Album", profile.album),
        default_field_mapping(format, "title", "Title", profile.title),
        default_field_mapping(format, "catalog", "Catalog", profile.catalog),
        default_field_mapping(format, "composer", "Composer", profile.composer),
        default_field_mapping(format, "genre", "Genre", profile.genre),
        default_field_mapping(format, "conductor", "Conductor", profile.conductor),
        default_field_mapping(format, "ensemble", "Ensemble", profile.ensemble),
        default_field_mapping(format, "soloist", "Soloist", profile.soloist),
        default_field_mapping(format, "year", "Year", profile.year),
        default_field_mapping(format, "disk_number", "Disk Number", profile.disk_number),
        default_field_mapping(format, "track_number", "Track Number", profile.track_number),
    ]
}

struct DefaultMappingProfile<'a> {
    album: &'a [&'a str],
    title: &'a [&'a str],
    catalog: &'a [&'a str],
    composer: &'a [&'a str],
    genre: &'a [&'a str],
    conductor: &'a [&'a str],
    ensemble: &'a [&'a str],
    soloist: &'a [&'a str],
    year: &'a [&'a str],
    disk_number: &'a [&'a str],
    track_number: &'a [&'a str],
}

fn default_mapping_profile(format: &str) -> DefaultMappingProfile<'static> {
    match canonical_field_mapping_format(format).as_str() {
        "DEFAULT" => default_fallback_mapping_profile(),
        "FLAC" => flac_mapping_profile(),
        "MP3" => mp3_mapping_profile(),
        "MP4" => mp4_mapping_profile(),
        "AAC" => aac_mapping_profile(),
        "OGG" => ogg_mapping_profile(),
        "OPUS" => opus_mapping_profile(),
        "WAV" => wav_mapping_profile(),
        "AIFF" => aiff_mapping_profile(),
        _ => default_fallback_mapping_profile(),
    }
}

fn default_fallback_mapping_profile() -> DefaultMappingProfile<'static> {
    DefaultMappingProfile {
        album: &["ALBUM"],
        title: &["TITLE"],
        catalog: &["CATALOGNUMBER", "CATALOG"],
        composer: &["COMPOSER", "WORKCOMPOSER", "COMPOSERSORT"],
        genre: &["GENRE"],
        conductor: &["CONDUCTOR"],
        ensemble: &["ENSEMBLE", "ORCHESTRA", "ALBUMARTIST"],
        soloist: &["SOLOIST", "PERFORMER", "ARTIST", "ALBUMARTIST"],
        year: &["DATE", "YEAR"],
        disk_number: &["DISCNUMBER", "DISKNUMBER", "DISC"],
        track_number: &["TRACKNUMBER", "TRACK"],
    }
}

fn flac_mapping_profile() -> DefaultMappingProfile<'static> {
    DefaultMappingProfile {
        album: &["ALBUM"],
        title: &["TITLE"],
        catalog: &["CATALOGNUMBER", "CATALOG"],
        composer: &["COMPOSER", "WORKCOMPOSER", "COMPOSERSORT"],
        genre: &["GENRE"],
        conductor: &["CONDUCTOR"],
        ensemble: &["ENSEMBLE", "ORCHESTRA", "ALBUMARTIST", "ARTIST"],
        soloist: &["SOLOIST", "PERFORMER", "ARTIST", "ALBUMARTIST"],
        year: &["DATE", "YEAR"],
        disk_number: &["DISCNUMBER", "DISKNUMBER", "DISC"],
        track_number: &["TRACKNUMBER", "TRACK"],
    }
}

fn mp3_mapping_profile() -> DefaultMappingProfile<'static> {
    DefaultMappingProfile {
        album: &["ALBUM"],
        title: &["TITLE"],
        catalog: &["CATALOGNUMBER", "CATALOG"],
        composer: &["COMPOSER", "WORKCOMPOSER", "COMPOSERSORT"],
        genre: &["GENRE"],
        conductor: &["CONDUCTOR"],
        ensemble: &["BAND", "ORCHESTRA", "ENSEMBLE", "ALBUMARTIST"],
        soloist: &["SOLOIST", "ARTIST", "PERFORMER", "ALBUMARTIST"],
        year: &["DATE", "YEAR"],
        disk_number: &["DISCNUMBER", "DISKNUMBER", "DISC"],
        track_number: &["TRACKNUMBER", "TRACK"],
    }
}

fn aac_mapping_profile() -> DefaultMappingProfile<'static> {
    DefaultMappingProfile {
        album: &["ALBUM"],
        title: &["TITLE"],
        catalog: &["CATALOGNUMBER", "CATALOG"],
        composer: &["COMPOSER", "WORKCOMPOSER", "COMPOSERSORT"],
        genre: &["GENRE"],
        conductor: &["CONDUCTOR"],
        ensemble: &["BAND", "ORCHESTRA", "ENSEMBLE", "ALBUMARTIST"],
        soloist: &["SOLOIST", "ARTIST", "PERFORMER", "ALBUMARTIST"],
        year: &["DATE", "YEAR"],
        disk_number: &["DISCNUMBER", "DISKNUMBER", "DISC"],
        track_number: &["TRACKNUMBER", "TRACK"],
    }
}

fn ogg_mapping_profile() -> DefaultMappingProfile<'static> {
    DefaultMappingProfile {
        album: &["ALBUM"],
        title: &["TITLE"],
        catalog: &["CATALOGNUMBER", "CATALOG"],
        composer: &["COMPOSER", "WORKCOMPOSER", "COMPOSERSORT"],
        genre: &["GENRE"],
        conductor: &["CONDUCTOR"],
        ensemble: &["ENSEMBLE", "ORCHESTRA", "ALBUMARTIST", "ARTIST"],
        soloist: &["SOLOIST", "PERFORMER", "ARTIST", "ALBUMARTIST"],
        year: &["DATE", "YEAR"],
        disk_number: &["DISCNUMBER", "DISKNUMBER", "DISC"],
        track_number: &["TRACKNUMBER", "TRACK"],
    }
}

fn opus_mapping_profile() -> DefaultMappingProfile<'static> {
    DefaultMappingProfile {
        album: &["ALBUM"],
        title: &["TITLE"],
        catalog: &["CATALOGNUMBER", "CATALOG"],
        composer: &["COMPOSER", "WORKCOMPOSER", "COMPOSERSORT"],
        genre: &["GENRE"],
        conductor: &["CONDUCTOR"],
        ensemble: &["ENSEMBLE", "ORCHESTRA", "ALBUMARTIST", "ARTIST"],
        soloist: &["SOLOIST", "PERFORMER", "ARTIST", "ALBUMARTIST"],
        year: &["DATE", "YEAR"],
        disk_number: &["DISCNUMBER", "DISKNUMBER", "DISC"],
        track_number: &["TRACKNUMBER", "TRACK"],
    }
}

fn wav_mapping_profile() -> DefaultMappingProfile<'static> {
    DefaultMappingProfile {
        album: &["ALBUM"],
        title: &["TITLE"],
        catalog: &["CATALOGNUMBER", "CATALOG"],
        composer: &["COMPOSER", "WORKCOMPOSER", "COMPOSERSORT"],
        genre: &["GENRE"],
        conductor: &["CONDUCTOR"],
        ensemble: &["BAND", "ORCHESTRA", "ENSEMBLE", "ALBUMARTIST"],
        soloist: &["SOLOIST", "ARTIST", "PERFORMER", "ALBUMARTIST"],
        year: &["DATE", "YEAR"],
        disk_number: &["DISCNUMBER", "DISKNUMBER", "DISC"],
        track_number: &["TRACKNUMBER", "TRACK"],
    }
}

fn aiff_mapping_profile() -> DefaultMappingProfile<'static> {
    DefaultMappingProfile {
        album: &["ALBUM"],
        title: &["TITLE"],
        catalog: &["CATALOGNUMBER", "CATALOG"],
        composer: &["COMPOSER", "WORKCOMPOSER", "COMPOSERSORT"],
        genre: &["GENRE"],
        conductor: &["CONDUCTOR"],
        ensemble: &["BAND", "ORCHESTRA", "ENSEMBLE", "ALBUMARTIST"],
        soloist: &["SOLOIST", "ARTIST", "PERFORMER", "ALBUMARTIST"],
        year: &["DATE", "YEAR"],
        disk_number: &["DISCNUMBER", "DISKNUMBER", "DISC"],
        track_number: &["TRACKNUMBER", "TRACK"],
    }
}

fn mp4_mapping_profile() -> DefaultMappingProfile<'static> {
    DefaultMappingProfile {
        album: &["©ALB", "ALBUM"],
        title: &["©NAM", "TITLE"],
        catalog: &[
            "----:com.apple.iTunes:CATALOGNUMBER",
            "----:com.apple.iTunes:CATALOG",
            "CATALOGNUMBER",
            "CATALOG",
        ],
        composer: &[
            "©WRT",
            "COMPOSER",
            "----:com.apple.iTunes:COMPOSER",
            "WORKCOMPOSER",
            "COMPOSERSORT",
        ],
        genre: &["©GEN", "GNRE", "GENRE"],
        conductor: &["----:com.apple.iTunes:CONDUCTOR", "CONDUCTOR"],
        ensemble: &[
            "AART",
            "----:com.apple.iTunes:ENSEMBLE",
            "----:com.apple.iTunes:ORCHESTRA",
            "ALBUMARTIST",
            "ENSEMBLE",
            "ORCHESTRA",
            "©ART",
            "ARTIST",
        ],
        soloist: &[
            "©ART",
            "ARTIST",
            "AART",
            "ALBUMARTIST",
            "----:com.apple.iTunes:SOLOIST",
            "----:com.apple.iTunes:PERFORMER",
            "SOLOIST",
            "PERFORMER",
        ],
        year: &["©DAY", "DATE", "YEAR"],
        disk_number: &["DISK", "DISCNUMBER", "DISKNUMBER", "DISC"],
        track_number: &["TRKN", "TRACKNUMBER", "TRACK"],
    }
}

fn default_field_mapping(
    format: &str,
    key: &str,
    label: &str,
    tag_priorities: &[&str],
) -> LibraryFieldMapping {
    LibraryFieldMapping {
        format: canonical_field_mapping_format(format),
        key: key.into(),
        label: label.into(),
        tag_priorities: tag_priorities.iter().map(|tag| (*tag).into()).collect(),
    }
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

#[cfg(test)]
mod tests {
    use super::default_field_mappings_for_format;

    #[test]
    fn mp4_defaults_include_freeform_classical_tags() {
        let mappings = default_field_mappings_for_format("mp4");

        let album = mappings
            .iter()
            .find(|mapping| mapping.key == "album")
            .expect("expected album mapping");
        let title = mappings
            .iter()
            .find(|mapping| mapping.key == "title")
            .expect("expected title mapping");
        let composer = mappings
            .iter()
            .find(|mapping| mapping.key == "composer")
            .expect("expected composer mapping");
        let year = mappings
            .iter()
            .find(|mapping| mapping.key == "year")
            .expect("expected year mapping");
        let track_number = mappings
            .iter()
            .find(|mapping| mapping.key == "track_number")
            .expect("expected track number mapping");
        let disk_number = mappings
            .iter()
            .find(|mapping| mapping.key == "disk_number")
            .expect("expected disk number mapping");
        let conductor = mappings
            .iter()
            .find(|mapping| mapping.key == "conductor")
            .expect("expected conductor mapping");
        let catalog = mappings
            .iter()
            .find(|mapping| mapping.key == "catalog")
            .expect("expected catalog mapping");

        assert_eq!(album.tag_priorities[0], "©ALB");
        assert_eq!(title.tag_priorities[0], "©NAM");
        assert_eq!(composer.tag_priorities[0], "©WRT");
        assert_eq!(year.tag_priorities[0], "©DAY");
        assert_eq!(track_number.tag_priorities[0], "TRKN");
        assert_eq!(disk_number.tag_priorities[0], "DISK");
        assert_eq!(
            conductor.tag_priorities[0],
            "----:com.apple.iTunes:CONDUCTOR"
        );
        assert_eq!(
            catalog.tag_priorities[0],
            "----:com.apple.iTunes:CATALOGNUMBER"
        );
    }

    #[test]
    fn id3_defaults_prioritize_band_for_ensemble() {
        let mappings = default_field_mappings_for_format("mp3");

        let ensemble = mappings
            .iter()
            .find(|mapping| mapping.key == "ensemble")
            .expect("expected ensemble mapping");

        assert_eq!(ensemble.tag_priorities[0], "BAND");
    }

    #[test]
    fn vorbis_defaults_include_track_and_disc_aliases() {
        let mappings = default_field_mappings_for_format("flac");

        let disk_number = mappings
            .iter()
            .find(|mapping| mapping.key == "disk_number")
            .expect("expected disk mapping");
        let track_number = mappings
            .iter()
            .find(|mapping| mapping.key == "track_number")
            .expect("expected track mapping");

        assert!(disk_number.tag_priorities.iter().any(|tag| tag == "DISKNUMBER"));
        assert!(track_number.tag_priorities.iter().any(|tag| tag == "TRACK"));
    }
}
