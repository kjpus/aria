use std::{collections::HashMap, fs, path::PathBuf};

use aria_domain::{
    canonical_field_mapping_format, default_catalog_rules, default_field_mappings, CatalogRule,
    LibraryFieldMapping, LibraryRoot, LibrarySnapshot, PlaybackSessionSnapshot,
    PlaylistSnapshot, ScannedTrack, SettingsSnapshot, TagInventoryEntry,
};
use rusqlite::{params, Connection};
use serde::de::DeserializeOwned;
use thiserror::Error;

const SETTINGS_KEY: &str = "settings";
const PLAYBACK_STATE_KEY: &str = "playback_session";
const PLAYLISTS_KEY: &str = "playlists";
const SNAPSHOT_ID: i64 = 1;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("failed to create Aria storage directory")]
    MissingStorageDir,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct PersistedState {
    pub library: LibrarySnapshot,
    pub playback: PlaybackSessionSnapshot,
    pub playlists: PlaylistSnapshot,
    pub settings: SettingsSnapshot,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            library: LibrarySnapshot::default(),
            playback: PlaybackSessionSnapshot::default(),
            playlists: PlaylistSnapshot::default(),
            settings: SettingsSnapshot::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppDatabase {
    path: PathBuf,
}

impl AppDatabase {
    pub fn new_default() -> Result<Self, StorageError> {
        let base_dir = dirs::data_local_dir()
            .or_else(|| std::env::current_dir().ok().map(|dir| dir.join(".cache")))
            .ok_or(StorageError::MissingStorageDir)?;
        let db_dir = base_dir.join("Aria");
        Self::at(db_dir.join("aria.sqlite3"))
    }

    pub fn at(path: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let database = Self { path };
        database.migrate()?;
        Ok(database)
    }

    pub fn load_state(&self) -> Result<PersistedState, StorageError> {
        let connection = self.connect()?;
        let settings = self.load_settings(&connection)?;
        let playback = self.load_playback_session(&connection)?;
        let playlists = self.load_playlist_snapshot(&connection)?;
        let library = self.load_library_snapshot(&connection)?;
        Ok(PersistedState {
            library,
            playback,
            playlists,
            settings,
        })
    }

    pub fn save_settings_snapshot(&self, snapshot: &SettingsSnapshot) -> Result<(), StorageError> {
        let connection = self.connect()?;
        let json = serde_json::to_string(snapshot)?;
        connection.execute(
            "insert into app_settings (setting_key, value_json)
             values (?1, ?2)
             on conflict(setting_key) do update set value_json = excluded.value_json",
            params![SETTINGS_KEY, json],
        )?;
        Ok(())
    }

    pub fn save_playback_session(
        &self,
        snapshot: &PlaybackSessionSnapshot,
    ) -> Result<(), StorageError> {
        let connection = self.connect()?;
        let json = serde_json::to_string(snapshot)?;
        connection.execute(
            "insert into app_settings (setting_key, value_json)
             values (?1, ?2)
             on conflict(setting_key) do update set value_json = excluded.value_json",
            params![PLAYBACK_STATE_KEY, json],
        )?;
        Ok(())
    }

    pub fn save_playlist_snapshot(&self, snapshot: &PlaylistSnapshot) -> Result<(), StorageError> {
        let connection = self.connect()?;
        let json = serde_json::to_string(snapshot)?;
        connection.execute(
            "insert into app_settings (setting_key, value_json)
             values (?1, ?2)
             on conflict(setting_key) do update set value_json = excluded.value_json",
            params![PLAYLISTS_KEY, json],
        )?;
        Ok(())
    }

    pub fn save_library_snapshot(&self, snapshot: &LibrarySnapshot) -> Result<(), StorageError> {
        let mut connection = self.connect()?;
        let tx = connection.transaction()?;

        tx.execute(
            "insert into library_state (snapshot_id, indexed_files, last_scan_at)
             values (?1, ?2, ?3)
             on conflict(snapshot_id) do update
             set indexed_files = excluded.indexed_files,
                 last_scan_at = excluded.last_scan_at",
            params![
                SNAPSHOT_ID,
                i64::try_from(snapshot.indexed_files).unwrap_or(i64::MAX),
                snapshot.last_scan_at.as_deref()
            ],
        )?;

        tx.execute("delete from library_roots", [])?;
        {
            let mut statement = tx
                .prepare("insert into library_roots (path, position, label) values (?1, ?2, ?3)")?;
            for (index, root) in snapshot.roots.iter().enumerate() {
                statement.execute(params![
                    root.path,
                    i64::try_from(index).unwrap_or(i64::MAX),
                    root.label
                ])?;
            }
        }

        tx.execute("delete from field_mappings", [])?;
        {
            let mut statement = tx.prepare(
                "insert into field_mappings (position, format, field_key, label, tag_priorities_json)
                 values (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (index, mapping) in snapshot.field_mappings.iter().enumerate() {
                statement.execute(params![
                    i64::try_from(index).unwrap_or(i64::MAX),
                    mapping.format,
                    mapping.key,
                    mapping.label,
                    serde_json::to_string(&mapping.tag_priorities)?,
                ])?;
            }
        }

        tx.execute("delete from catalog_rules", [])?;
        {
            let mut statement = tx.prepare(
                "insert into catalog_rules (
                   position, label, pattern, composers_json, source_tags_json, enabled
                 ) values (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (index, rule) in snapshot.catalog_rules.iter().enumerate() {
                statement.execute(params![
                    i64::try_from(index).unwrap_or(i64::MAX),
                    rule.label,
                    legacy_catalog_pattern(&rule.label),
                    serde_json::to_string(&rule.composers)?,
                    catalog_source_tags_json(),
                    if rule.enabled { 1 } else { 0 }
                ])?;
            }
        }

        tx.execute("delete from tag_inventory", [])?;
        {
            let mut statement = tx.prepare(
                "insert into tag_inventory (tag, occurrences, example_values_json)
                 values (?1, ?2, ?3)",
            )?;
            for entry in &snapshot.tag_inventory {
                statement.execute(params![
                    entry.tag,
                    i64::try_from(entry.occurrences).unwrap_or(i64::MAX),
                    serde_json::to_string(&entry.example_values)?
                ])?;
            }
        }

        tx.execute("delete from scanned_tracks", [])?;
        {
            let mut statement = tx.prepare(
                "insert into scanned_tracks (
                   id, path, file_name, album_art_path, audio_json, raw_tags_json, mapped_fields_json
                 ) values (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for track in &snapshot.tracks {
                statement.execute(params![
                    track.id,
                    track.path,
                    track.file_name,
                    track.album_art_path.as_deref(),
                    serde_json::to_string(&track.audio)?,
                    serde_json::to_string(&track.raw_tags)?,
                    serde_json::to_string(&track.mapped_fields)?
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), StorageError> {
        let connection = self.connect()?;
        connection.execute_batch(include_str!("../migrations/0001_initial.sql"))?;
        self.ensure_field_mapping_format_column(&connection)?;
        Ok(())
    }

    fn ensure_field_mapping_format_column(
        &self,
        connection: &Connection,
    ) -> Result<(), StorageError> {
        let has_format_column: i64 = connection.query_row(
            "select count(*) from pragma_table_info('field_mappings') where name = 'format'",
            [],
            |row| row.get(0),
        )?;

        if has_format_column == 0 {
            connection.execute(
                "alter table field_mappings add column format text not null default 'DEFAULT'",
                [],
            )?;
        }

        Ok(())
    }

    fn connect(&self) -> Result<Connection, StorageError> {
        Ok(Connection::open(&self.path)?)
    }

    fn load_settings(&self, connection: &Connection) -> Result<SettingsSnapshot, StorageError> {
        load_setting_json(connection, SETTINGS_KEY)
    }

    fn load_playback_session(
        &self,
        connection: &Connection,
    ) -> Result<PlaybackSessionSnapshot, StorageError> {
        load_setting_json(connection, PLAYBACK_STATE_KEY)
    }

    fn load_playlist_snapshot(
        &self,
        connection: &Connection,
    ) -> Result<PlaylistSnapshot, StorageError> {
        load_setting_json(connection, PLAYLISTS_KEY)
    }

    fn load_library_snapshot(
        &self,
        connection: &Connection,
    ) -> Result<LibrarySnapshot, StorageError> {
        let mut snapshot = LibrarySnapshot::default();

        {
            let mut statement = connection.prepare(
                "select indexed_files, last_scan_at from library_state where snapshot_id = ?1",
            )?;
            let mut rows = statement.query(params![SNAPSHOT_ID])?;
            if let Some(row) = rows.next()? {
                snapshot.indexed_files = row.get::<_, i64>(0)?.max(0) as u64;
                snapshot.last_scan_at = row.get(1)?;
            }
        }

        snapshot.roots = self.load_library_roots(connection)?;
        snapshot.field_mappings = upgrade_loaded_field_mappings(self.load_field_mappings(connection)?);
        let catalog_rules = self.load_catalog_rules(connection)?;
        if !catalog_rules.is_empty() {
            snapshot.catalog_rules = catalog_rules;
        }
        snapshot.tag_inventory = self.load_tag_inventory(connection)?;
        snapshot.tracks = self.load_scanned_tracks(connection)?;
        snapshot.is_scanning = false;

        if snapshot.field_mappings.is_empty() {
            snapshot.field_mappings = default_field_mappings();
        }

        if snapshot.catalog_rules.is_empty() {
            snapshot.catalog_rules = default_catalog_rules();
        }

        Ok(snapshot)
    }

    fn load_library_roots(
        &self,
        connection: &Connection,
    ) -> Result<Vec<LibraryRoot>, StorageError> {
        let mut statement = connection
            .prepare("select path, label from library_roots order by position asc, path asc")?;
        let rows = statement.query_map([], |row| {
            Ok(LibraryRoot {
                path: row.get(0)?,
                label: row.get(1)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    fn load_field_mappings(
        &self,
        connection: &Connection,
    ) -> Result<Vec<LibraryFieldMapping>, StorageError> {
        let mut statement = connection.prepare(
            "select format, field_key, label, tag_priorities_json
             from field_mappings
             order by position asc",
        )?;
        let rows = statement.query_map([], |row| {
            let format: String = row.get(0)?;
            let tag_priorities_json: String = row.get(3)?;
            let tag_priorities = serde_json::from_str(&tag_priorities_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;

            Ok(LibraryFieldMapping {
                format: canonical_field_mapping_format(&format),
                key: row.get(1)?,
                label: row.get(2)?,
                tag_priorities,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    fn load_catalog_rules(&self, connection: &Connection) -> Result<Vec<CatalogRule>, StorageError> {
        let mut statement = connection.prepare(
            "select label, composers_json, enabled
             from catalog_rules
             order by position asc",
        )?;
        let rows = statement.query_map([], |row| {
            let composers_json: String = row.get(1)?;
            let composers = serde_json::from_str(&composers_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;

            Ok(CatalogRule {
                label: row.get(0)?,
                composers,
                enabled: row.get::<_, i64>(2)? != 0,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    fn load_tag_inventory(
        &self,
        connection: &Connection,
    ) -> Result<Vec<TagInventoryEntry>, StorageError> {
        let mut statement = connection.prepare(
            "select tag, occurrences, example_values_json
             from tag_inventory
             order by occurrences desc, tag asc",
        )?;
        let rows = statement.query_map([], |row| {
            let example_values_json: String = row.get(2)?;
            let example_values = serde_json::from_str(&example_values_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;

            Ok(TagInventoryEntry {
                tag: row.get(0)?,
                occurrences: row.get::<_, i64>(1)?.max(0) as u64,
                example_values,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    fn load_scanned_tracks(
        &self,
        connection: &Connection,
    ) -> Result<Vec<ScannedTrack>, StorageError> {
        let mut statement = connection.prepare(
            "select id, path, file_name, album_art_path, audio_json, raw_tags_json, mapped_fields_json
             from scanned_tracks
             order by path asc",
        )?;
        let rows = statement.query_map([], |row| {
            let audio_json: String = row.get(4)?;
            let raw_tags_json: String = row.get(5)?;
            let mapped_fields_json: String = row.get(6)?;

            Ok(ScannedTrack {
                id: row.get(0)?,
                path: row.get(1)?,
                file_name: row.get(2)?,
                album_art_path: row.get(3)?,
                audio: from_json_column(4, &audio_json)?,
                raw_tags: from_json_column(5, &raw_tags_json)?,
                mapped_fields: from_json_column(6, &mapped_fields_json)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }
}

fn from_json_column<T: DeserializeOwned>(
    column_index: usize,
    json: &str,
) -> Result<T, rusqlite::Error> {
    serde_json::from_str(json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            column_index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn load_setting_json<T>(connection: &Connection, key: &str) -> Result<T, StorageError>
where
    T: DeserializeOwned + Default,
{
    let mut statement =
        connection.prepare("select value_json from app_settings where setting_key = ?1 limit 1")?;
    let mut rows = statement.query(params![key])?;
    if let Some(row) = rows.next()? {
        let json: String = row.get(0)?;
        Ok(serde_json::from_str(&json)?)
    } else {
        Ok(T::default())
    }
}

fn catalog_source_tags_json() -> &'static str {
    r#"["TITLE","WORK","ALBUM"]"#
}

fn legacy_catalog_pattern(label: &str) -> String {
    let trimmed = label.trim();
    let prefix = if trimmed.eq_ignore_ascii_case("op") || trimmed.eq_ignore_ascii_case("opus") {
        String::from(r"(?:Op\.?|Opus)")
    } else {
        format!(r"{}\.?", legacy_regex_escape(trimmed.trim_end_matches('.')))
    };

    format!(
        r"(?i)\b{prefix}\s*(?:[IVXLCM]+\s*[:.]\s*)?\d+[A-Za-z]?(?:\s*[:.]\s*[A-Za-z0-9]+)?(?:\s*No\.?\s*\d+)?\b"
    )
}

fn legacy_regex_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$'
            | '|' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }

    escaped
}

fn upgrade_loaded_field_mappings(mappings: Vec<LibraryFieldMapping>) -> Vec<LibraryFieldMapping> {
    if mappings.is_empty() {
        return default_field_mappings();
    }

    if is_legacy_default_only_mapping_set(&mappings) {
        return default_field_mappings();
    }

    let current_defaults = default_mapping_lookup(default_field_mappings());
    let legacy_defaults = default_mapping_lookup(legacy_per_format_field_mappings());

    mappings
        .into_iter()
        .map(|mapping| {
            let format = canonical_field_mapping_format(&mapping.format);
            let key = (format.clone(), mapping.key.clone());

            match current_defaults.get(&key) {
                Some(current_default)
                    if legacy_defaults
                        .get(&key)
                        .is_some_and(|legacy| mapping.label == legacy.label
                            && mapping.tag_priorities == legacy.tag_priorities)
                        || is_historical_mp4_composer_default(&format, &mapping) =>
                {
                    current_default.clone()
                }
                _ => LibraryFieldMapping { format, ..mapping },
            }
        })
        .collect()
}

fn is_historical_mp4_composer_default(format: &str, mapping: &LibraryFieldMapping) -> bool {
    const PRE_WRT_MP4_COMPOSER_TAGS: &[&str] = &["COMPOSER", "WORKCOMPOSER", "COMPOSERSORT"];
    const PRE_WRT_WITH_FREEFORM_MP4_COMPOSER_TAGS: &[&str] = &[
        "COMPOSER",
        "----:com.apple.iTunes:COMPOSER",
        "WORKCOMPOSER",
        "COMPOSERSORT",
    ];

    format == "MP4"
        && mapping.key == "composer"
        && mapping.label == "Composer"
        && matches_historical_tag_priorities(&mapping.tag_priorities, PRE_WRT_MP4_COMPOSER_TAGS)
            || format == "MP4"
                && mapping.key == "composer"
                && mapping.label == "Composer"
                && matches_historical_tag_priorities(
                    &mapping.tag_priorities,
                    PRE_WRT_WITH_FREEFORM_MP4_COMPOSER_TAGS,
                )
}

fn matches_historical_tag_priorities(
    actual: &[String],
    expected: &[&str],
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected.iter())
            .all(|(tag, expected_tag)| tag == expected_tag)
}

fn is_legacy_default_only_mapping_set(mappings: &[LibraryFieldMapping]) -> bool {
    if mappings.is_empty() {
        return false;
    }

    if mappings
        .iter()
        .any(|mapping| canonical_field_mapping_format(&mapping.format) != "DEFAULT")
    {
        return false;
    }

    let legacy_defaults = default_mapping_lookup(legacy_default_only_field_mappings());

    mappings.len() == legacy_defaults.len()
        && mappings.iter().all(|mapping| {
            let key = (
                canonical_field_mapping_format(&mapping.format),
                mapping.key.clone(),
            );

            legacy_defaults.get(&key).is_some_and(|legacy| {
                mapping.label == legacy.label && mapping.tag_priorities == legacy.tag_priorities
            })
        })
}

fn default_mapping_lookup(
    mappings: Vec<LibraryFieldMapping>,
) -> HashMap<(String, String), LibraryFieldMapping> {
    mappings
        .into_iter()
        .map(|mapping| {
            (
                (
                    canonical_field_mapping_format(&mapping.format),
                    mapping.key.clone(),
                ),
                mapping,
            )
        })
        .collect()
}

fn legacy_default_only_field_mappings() -> Vec<LibraryFieldMapping> {
    legacy_field_mappings_for_format("DEFAULT")
}

fn legacy_per_format_field_mappings() -> Vec<LibraryFieldMapping> {
    let mut mappings = Vec::new();

    for format in ["DEFAULT", "FLAC", "MP3", "MP4", "AAC", "OGG", "OPUS", "WAV", "AIFF"] {
        mappings.extend(legacy_field_mappings_for_format(format));
    }

    mappings
}

fn legacy_field_mappings_for_format(format: &str) -> Vec<LibraryFieldMapping> {
    let normalized_format = canonical_field_mapping_format(format);
    let catalog = if normalized_format == "MP4" {
        vec![
            "----:com.apple.iTunes:CATALOGNUMBER".into(),
            "----:com.apple.iTunes:CATALOG".into(),
            "CATALOGNUMBER".into(),
            "CATALOG".into(),
        ]
    } else {
        vec!["CATALOGNUMBER".into(), "CATALOG".into()]
    };

    vec![
        legacy_field_mapping(&normalized_format, "album", "Album", &["ALBUM"]),
        legacy_field_mapping(&normalized_format, "title", "Title", &["TITLE"]),
        LibraryFieldMapping {
            format: normalized_format.clone(),
            key: "catalog".into(),
            label: "Catalog".into(),
            tag_priorities: catalog,
        },
        legacy_field_mapping(&normalized_format, "composer", "Composer", &["COMPOSER"]),
        legacy_field_mapping(&normalized_format, "genre", "Genre", &["GENRE"]),
        legacy_field_mapping(&normalized_format, "conductor", "Conductor", &["CONDUCTOR"]),
        legacy_field_mapping(
            &normalized_format,
            "ensemble",
            "Ensemble",
            &["ENSEMBLE", "ORCHESTRA", "ALBUMARTIST"],
        ),
        legacy_field_mapping(
            &normalized_format,
            "soloist",
            "Soloist",
            &["PERFORMER", "ARTIST", "ALBUMARTIST"],
        ),
        legacy_field_mapping(&normalized_format, "year", "Year", &["DATE", "YEAR"]),
        legacy_field_mapping(
            &normalized_format,
            "disk_number",
            "Disk Number",
            &["DISCNUMBER"],
        ),
        legacy_field_mapping(
            &normalized_format,
            "track_number",
            "Track Number",
            &["TRACKNUMBER"],
        ),
    ]
}

fn legacy_field_mapping(
    format: &str,
    key: &str,
    label: &str,
    tag_priorities: &[&str],
) -> LibraryFieldMapping {
    LibraryFieldMapping {
        format: format.into(),
        key: key.into(),
        label: label.into(),
        tag_priorities: tag_priorities.iter().map(|tag| (*tag).into()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use aria_domain::{
        default_field_mappings, CatalogRule, LibraryFieldMapping, LibrarySnapshot, PlayTrackRequest, PlaybackPreferences,
        PlaybackSessionSnapshot, QueueItem, SettingsSnapshot, ThemePreference, TrackTableSettings,
    };

    use super::{
        legacy_default_only_field_mappings, legacy_field_mappings_for_format,
        upgrade_loaded_field_mappings, AppDatabase,
    };

    #[test]
    fn persists_library_and_settings_across_reloads() {
        let test_root = std::env::temp_dir().join(format!(
            "aria-storage-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after epoch")
                .as_nanos()
        ));

        let database =
            AppDatabase::at(test_root.join("aria.sqlite3")).expect("database should initialize");

        let mut library = LibrarySnapshot::default();
        library.roots.push(aria_domain::LibraryRoot {
            path: r"C:\Music\Test".into(),
            label: "Test".into(),
        });
        library.indexed_files = 1;
        library.last_scan_at = Some("2026-04-06T10:00:00Z".into());
        library.catalog_rules = vec![CatalogRule {
            label: "WAB".into(),
            composers: vec!["Anton Bruckner".into(), "Bruckner".into()],
            enabled: true,
        }];
        library.tag_inventory.push(aria_domain::TagInventoryEntry {
            tag: "COMPOSER".into(),
            occurrences: 1,
            example_values: vec!["Mozart".into()],
        });
        library.tracks.push(aria_domain::ScannedTrack {
            id: "track-1".into(),
            path: r"C:\Music\Test\disc1\track1.flac".into(),
            file_name: "track1.flac".into(),
            album_art_path: Some(r"C:\Music\Test\cover.jpg".into()),
            audio: aria_domain::AudioPropertiesSnapshot {
                format: "FLAC".into(),
                duration_ms: 123_456,
                sample_rate: Some(96_000),
                bit_depth: Some(24),
                channels: Some(2),
            },
            raw_tags: BTreeMap::from([
                ("ALBUM".into(), vec!["Test Album".into()]),
                ("TITLE".into(), vec!["Allegro".into()]),
            ]),
            mapped_fields: BTreeMap::from([
                ("album".into(), vec!["Test Album".into()]),
                ("title".into(), vec!["Allegro".into()]),
            ]),
        });

        let settings = SettingsSnapshot {
            theme: ThemePreference::Dark,
            accent_color: "#abcdef".into(),
            track_table: TrackTableSettings::default(),
            album_track_table: aria_domain::default_album_track_table_settings(),
            playlist_track_table: aria_domain::default_playlist_track_table_settings(),
            playback: PlaybackPreferences {
                output_device_id: Some("wasapi:test-device".into()),
                exclusive_mode: false,
                volume: 1.0,
            },
        };
        let playback = PlaybackSessionSnapshot {
            queue: vec![PlayTrackRequest {
                path: r"C:\Music\Test\disc1\track1.flac".into(),
                queue_item: QueueItem {
                    id: "track-1".into(),
                    title: "Allegro".into(),
                    subtitle: "Test Performer".into(),
                    duration_ms: 123_456,
                },
            }],
            ordered_queue: vec![PlayTrackRequest {
                path: r"C:\Music\Test\disc1\track1.flac".into(),
                queue_item: QueueItem {
                    id: "track-1".into(),
                    title: "Allegro".into(),
                    subtitle: "Test Performer".into(),
                    duration_ms: 123_456,
                },
            }],
            current_queue_index: Some(0),
        };

        database
            .save_library_snapshot(&library)
            .expect("library snapshot should save");
        database
            .save_playback_session(&playback)
            .expect("playback session should save");
        database
            .save_settings_snapshot(&settings)
            .expect("settings should save");

        let loaded = database.load_state().expect("state should load");

        assert_eq!(loaded.settings.accent_color, "#abcdef");
        assert!(matches!(loaded.settings.theme, ThemePreference::Dark));
        assert_eq!(
            loaded.settings.playback.output_device_id.as_deref(),
            Some("wasapi:test-device")
        );
        assert_eq!(loaded.library.roots.len(), 1);
        assert_eq!(loaded.library.roots[0].path, r"C:\Music\Test");
        assert_eq!(loaded.library.indexed_files, 1);
        assert_eq!(loaded.library.tag_inventory.len(), 1);
        assert_eq!(loaded.library.tag_inventory[0].tag, "COMPOSER");
        assert_eq!(loaded.library.catalog_rules.len(), 1);
        assert_eq!(loaded.library.catalog_rules[0].label, "WAB");
        assert_eq!(loaded.library.tracks.len(), 1);
        assert_eq!(
            loaded.library.tracks[0].mapped_fields["title"][0],
            "Allegro"
        );
        assert!(!loaded.library.is_scanning);
        assert_eq!(loaded.playback.queue.len(), 1);
        assert_eq!(loaded.playback.current_queue_index, Some(0));
        assert_eq!(loaded.playback.queue[0].queue_item.id, "track-1");

        fs::remove_dir_all(&test_root).ok();
    }

    #[test]
    fn upgrades_legacy_default_only_mappings_to_full_format_defaults() {
        let upgraded = upgrade_loaded_field_mappings(legacy_default_only_field_mappings());

        assert_eq!(upgraded.len(), default_field_mappings().len());
        assert!(upgraded.iter().any(|mapping| mapping.format == "FLAC"));

        let mp4_conductor = upgraded
            .iter()
            .find(|mapping| mapping.format == "MP4" && mapping.key == "conductor")
            .expect("expected MP4 conductor mapping");

        assert_eq!(
            mp4_conductor.tag_priorities[0],
            "----:com.apple.iTunes:CONDUCTOR"
        );
    }

    #[test]
    fn upgrades_legacy_per_format_defaults_without_overwriting_custom_tags() {
        let mut legacy_mp4 = legacy_field_mappings_for_format("MP4");
        let soloist = legacy_mp4
            .iter_mut()
            .find(|mapping| mapping.key == "soloist")
            .expect("expected soloist mapping");
        soloist.tag_priorities = vec!["CUSTOMSOLOIST".into(), "ARTIST".into()];

        let upgraded = upgrade_loaded_field_mappings(legacy_mp4);

        let conductor = upgraded
            .iter()
            .find(|mapping| mapping.key == "conductor")
            .expect("expected conductor mapping");
        let soloist = upgraded
            .iter()
            .find(|mapping| mapping.key == "soloist")
            .expect("expected soloist mapping");

        assert_eq!(
            conductor.tag_priorities[0],
            "----:com.apple.iTunes:CONDUCTOR"
        );
        assert_eq!(
            soloist,
            &LibraryFieldMapping {
                format: "MP4".into(),
                key: "soloist".into(),
                label: "Soloist".into(),
                tag_priorities: vec!["CUSTOMSOLOIST".into(), "ARTIST".into()],
            }
        );
    }

    #[test]
    fn upgrades_pre_wrt_mp4_composer_defaults() {
        let mut stored_defaults = default_field_mappings();
        let composer = stored_defaults
            .iter_mut()
            .find(|mapping| mapping.format == "MP4" && mapping.key == "composer")
            .expect("expected MP4 composer mapping");
        composer.tag_priorities = vec![
            "COMPOSER".into(),
            "WORKCOMPOSER".into(),
            "COMPOSERSORT".into(),
        ];

        let upgraded = upgrade_loaded_field_mappings(stored_defaults);

        let composer = upgraded
            .iter()
            .find(|mapping| mapping.format == "MP4" && mapping.key == "composer")
            .expect("expected upgraded MP4 composer mapping");

        assert_eq!(composer.tag_priorities[0], "©WRT");
        assert_eq!(
            composer.tag_priorities,
            vec![
                "©WRT".to_string(),
                "COMPOSER".to_string(),
                "----:com.apple.iTunes:COMPOSER".to_string(),
                "WORKCOMPOSER".to_string(),
                "COMPOSERSORT".to_string(),
            ]
        );
    }

    #[test]
    fn upgrades_pre_wrt_mp4_composer_defaults_with_freeform_composer() {
        let mut stored_defaults = default_field_mappings();
        let composer = stored_defaults
            .iter_mut()
            .find(|mapping| mapping.format == "MP4" && mapping.key == "composer")
            .expect("expected MP4 composer mapping");
        composer.tag_priorities = vec![
            "COMPOSER".into(),
            "----:com.apple.iTunes:COMPOSER".into(),
            "WORKCOMPOSER".into(),
            "COMPOSERSORT".into(),
        ];

        let upgraded = upgrade_loaded_field_mappings(stored_defaults);

        let composer = upgraded
            .iter()
            .find(|mapping| mapping.format == "MP4" && mapping.key == "composer")
            .expect("expected upgraded MP4 composer mapping");

        assert_eq!(composer.tag_priorities[0], "©WRT");
        assert_eq!(
            composer.tag_priorities,
            vec![
                "©WRT".to_string(),
                "COMPOSER".to_string(),
                "----:com.apple.iTunes:COMPOSER".to_string(),
                "WORKCOMPOSER".to_string(),
                "COMPOSERSORT".to_string(),
            ]
        );
    }
}
