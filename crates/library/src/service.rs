use std::{
    collections::{BTreeMap, HashMap},
    fs::File as StdFile,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

use aria_domain::{
    default_catalog_rules, AppEvent, AudioPropertiesSnapshot, CatalogRule, LibraryEvent,
    LibraryFieldMapping, LibraryRoot, LibrarySnapshot, ScanProgress, ScannedTrack, TagInventoryEntry,
};
use chrono::Utc;
use lofty::{
    config::ParseOptions,
    file::{AudioFile, TaggedFileExt},
    flac::FlacFile,
    ogg::{OggPictureStorage, OpusFile, SpeexFile, VorbisComments, VorbisFile},
    picture::{MimeType, Picture, PictureType},
    read_from_path,
    tag::{ItemKey, ItemValue, TagType},
};
use regex::Regex;
use tokio::{
    sync::{broadcast, RwLock},
    task::spawn_blocking,
};
use walkdir::WalkDir;

use crate::LibraryError;

#[derive(Clone, Default)]
pub struct LibraryService {
    state: Arc<RwLock<LibrarySnapshot>>,
}

#[derive(Default)]
struct InventoryAccumulator {
    occurrences: u64,
    example_values: Vec<String>,
}

struct ScanArtifacts {
    tracks: Vec<ScannedTrack>,
    tag_inventory: Vec<TagInventoryEntry>,
    indexed_files: u64,
}

struct CompiledCatalogRule {
    regex: Regex,
    composer_hints: Vec<String>,
}

const DEFAULT_CATALOG_SOURCE_TAGS: &[&str] = &["TITLE", "WORK", "ALBUM"];
const COMPOSER_SOURCE_TAGS: &[&str] = &["COMPOSER", "WORKCOMPOSER", "COMPOSERSORT"];

impl LibraryService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_snapshot(mut snapshot: LibrarySnapshot) -> Self {
        snapshot.is_scanning = false;
        if snapshot.catalog_rules.is_empty() {
            snapshot.catalog_rules = default_catalog_rules();
        }
        Self {
            state: Arc::new(RwLock::new(snapshot)),
        }
    }

    pub async fn snapshot(&self) -> LibrarySnapshot {
        self.state.read().await.clone()
    }

    pub async fn add_root(&self, path: String) -> Result<LibrarySnapshot, LibraryError> {
        let normalized = normalize_root_path(&path);
        if normalized.is_empty() {
            return Err(LibraryError::EmptyRoot);
        }

        let label = Path::new(&normalized)
            .file_name()
            .and_then(|segment| segment.to_str())
            .map(|segment| segment.to_string())
            .unwrap_or_else(|| normalized.clone());

        let mut state = self.state.write().await;
        if state
            .roots
            .iter()
            .any(|root| root_paths_equal(&root.path, &normalized))
        {
            return Err(LibraryError::DuplicateRoot);
        }

        state.roots.push(LibraryRoot {
            path: normalized,
            label,
        });

        Ok(state.clone())
    }

    pub async fn remove_root(&self, path: String) -> LibrarySnapshot {
        let normalized = normalize_root_path(&path);
        let mut state = self.state.write().await;
        state.roots
            .retain(|root| !root_paths_equal(&root.path, &normalized));
        state.clone()
    }

    pub async fn clear(&self) -> LibrarySnapshot {
        let mut state = self.state.write().await;
        state.is_scanning = false;
        state.roots.clear();
        state.indexed_files = 0;
        state.last_scan_at = None;
        state.tag_inventory.clear();
        state.tracks.clear();
        state.clone()
    }

    pub async fn set_field_mappings(&self, mappings: Vec<LibraryFieldMapping>) -> LibrarySnapshot {
        let mut state = self.state.write().await;
        state.field_mappings = mappings
            .into_iter()
            .filter(|mapping| !mapping.key.trim().is_empty() && !mapping.label.trim().is_empty())
            .map(|mut mapping| {
                mapping.tag_priorities = mapping
                    .tag_priorities
                    .into_iter()
                    .map(|tag| normalize_tag_name(&tag))
                    .filter(|tag| !tag.is_empty())
                    .collect();
                mapping
            })
            .collect();

        let mappings = state.field_mappings.clone();
        let catalog_rules = compile_catalog_rules(&state.catalog_rules);
        for track in &mut state.tracks {
            track.mapped_fields = map_fields(&track.raw_tags, &mappings, &catalog_rules);
        }

        state.clone()
    }

    pub async fn set_catalog_rules(
        &self,
        rules: Vec<CatalogRule>,
    ) -> Result<LibrarySnapshot, LibraryError> {
        let sanitized_rules = sanitize_catalog_rules(rules);
        validate_catalog_rules(&sanitized_rules)?;

        let mut state = self.state.write().await;
        state.catalog_rules = sanitized_rules;

        let mappings = state.field_mappings.clone();
        let catalog_rules = compile_catalog_rules(&state.catalog_rules);
        for track in &mut state.tracks {
            track.mapped_fields = map_fields(&track.raw_tags, &mappings, &catalog_rules);
        }

        Ok(state.clone())
    }

    pub async fn start_scan(
        &self,
        events: broadcast::Sender<AppEvent>,
    ) -> Result<(), LibraryError> {
        let mut state = self.state.write().await;
        if state.is_scanning {
            return Err(LibraryError::AlreadyScanning);
        }

        state.is_scanning = true;
        let started_snapshot = state.clone();
        let roots = state.roots.clone();
        let mappings = state.field_mappings.clone();
        let catalog_rules = state.catalog_rules.clone();
        drop(state);

        let _ = events.send(AppEvent::Library(LibraryEvent::SnapshotChanged(
            started_snapshot,
        )));

        let state = self.state.clone();
        tokio::spawn(async move {
            let cache_root = album_art_cache_root();

            let progress_events = events.clone();
            let scan_result = spawn_blocking(move || {
                scan_library(
                    &roots,
                    &mappings,
                    &catalog_rules,
                    &cache_root,
                    &progress_events,
                )
            })
            .await;

            match scan_result {
                Ok(Ok(artifacts)) => {
                    let completed_snapshot = {
                        let mut state = state.write().await;
                        state.is_scanning = false;
                        state.indexed_files = artifacts.indexed_files;
                        state.last_scan_at = Some(Utc::now().to_rfc3339());
                        state.tag_inventory = artifacts.tag_inventory;
                        state.tracks = artifacts.tracks;
                        state.clone()
                    };

                    let _ = events.send(AppEvent::Library(LibraryEvent::SnapshotChanged(
                        completed_snapshot,
                    )));
                }
                _ => {
                    let failed_snapshot = {
                        let mut state = state.write().await;
                        state.is_scanning = false;
                        state.clone()
                    };

                    let _ = events.send(AppEvent::Library(LibraryEvent::SnapshotChanged(
                        failed_snapshot,
                    )));
                }
            }
        });

        Ok(())
    }

    pub async fn read_track_raw_tags(
        &self,
        path: String,
    ) -> Result<BTreeMap<String, Vec<String>>, LibraryError> {
        let requested_path = PathBuf::from(path);
        spawn_blocking(move || read_raw_tags_from_path(&requested_path))
            .await
            .map_err(|_| LibraryError::TrackReadFailure)?
    }
}

fn scan_library(
    roots: &[LibraryRoot],
    mappings: &[LibraryFieldMapping],
    catalog_rules: &[CatalogRule],
    cache_root: &Path,
    events: &broadcast::Sender<AppEvent>,
) -> Result<ScanArtifacts, LibraryError> {
    let compiled_catalog_rules = compile_catalog_rules(catalog_rules);
    let audio_files = discover_audio_files(roots);
    let discovered_files = audio_files.len() as u64;
    let mut failed_files = 0_u64;
    let mut tracks = Vec::new();
    let mut inventory: HashMap<String, InventoryAccumulator> = HashMap::new();

    std::fs::create_dir_all(cache_root).ok();

    for (index, path) in audio_files.iter().enumerate() {
        match scan_single_track(path, mappings, &compiled_catalog_rules, cache_root) {
            Ok(track) => {
                merge_inventory(&mut inventory, &track.raw_tags);
                tracks.push(track);
            }
            Err(_) => {
                failed_files += 1;
            }
        }

        let processed_files = (index + 1) as u64;
        if processed_files == discovered_files || processed_files % 25 == 0 {
            let _ = events.send(AppEvent::Library(LibraryEvent::ScanProgress(
                ScanProgress {
                    phase: "indexing".into(),
                    processed_files,
                    discovered_files,
                    failed_files,
                },
            )));
        }
    }

    tracks.sort_by(|left, right| left.path.cmp(&right.path));

    let mut tag_inventory = inventory
        .into_iter()
        .map(|(tag, accumulator)| TagInventoryEntry {
            tag,
            occurrences: accumulator.occurrences,
            example_values: accumulator.example_values,
        })
        .collect::<Vec<_>>();

    tag_inventory.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then_with(|| left.tag.cmp(&right.tag))
    });

    Ok(ScanArtifacts {
        indexed_files: tracks.len() as u64,
        tracks,
        tag_inventory,
    })
}

fn album_art_cache_root() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Aria")
        .join("album-art")
}

fn discover_audio_files(roots: &[LibraryRoot]) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for root in roots {
        for entry in WalkDir::new(&root.path)
            .follow_links(true)
            .into_iter()
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if entry.file_type().is_file() && is_supported_audio_file(path) {
                files.push(path.to_path_buf());
            }
        }
    }

    files
}

fn is_supported_audio_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase()),
        Some(ext)
            if matches!(
                ext.as_str(),
                "flac" | "mp3" | "m4a" | "aac" | "mp4" | "ogg" | "opus" | "wav" | "aiff" | "aif"
            )
    )
}

fn scan_single_track(
    path: &Path,
    mappings: &[LibraryFieldMapping],
    catalog_rules: &[CompiledCatalogRule],
    cache_root: &Path,
) -> Result<ScannedTrack, LibraryError> {
    let tagged_file = read_from_path(path).map_err(|_| LibraryError::ScanFailure)?;
    let mut raw_tags = collect_raw_tags(&tagged_file);
    merge_format_specific_raw_tags(path, &mut raw_tags);
    let mapped_fields = map_fields(&raw_tags, mappings, catalog_rules);
    let album_art_path = find_album_art(path, cache_root);
    let properties = tagged_file.properties();

    Ok(ScannedTrack {
        id: path.to_string_lossy().into_owned(),
        path: path.to_string_lossy().into_owned(),
        file_name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string(),
        album_art_path: album_art_path.map(|art| art.to_string_lossy().into_owned()),
        audio: AudioPropertiesSnapshot {
            format: path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_uppercase())
                .unwrap_or_else(|| "UNKNOWN".into()),
            duration_ms: properties.duration().as_millis() as u64,
            sample_rate: properties.sample_rate(),
            bit_depth: properties.bit_depth(),
            channels: properties.channels(),
        },
        raw_tags,
        mapped_fields,
    })
}

fn merge_format_specific_raw_tags(path: &Path, raw_tags: &mut BTreeMap<String, Vec<String>>) {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase());

    match extension.as_deref() {
        Some("flac") => {
            if let Some(vorbis_comments) = read_flac_vorbis_comments(path) {
                merge_vorbis_comments(raw_tags, &vorbis_comments);
            }
        }
        Some("ogg") => {
            if let Some(vorbis_comments) = read_ogg_vorbis_comments(path) {
                merge_vorbis_comments(raw_tags, &vorbis_comments);
            }
        }
        Some("opus") => {
            if let Some(vorbis_comments) = read_opus_vorbis_comments(path) {
                merge_vorbis_comments(raw_tags, &vorbis_comments);
            }
        }
        Some("spx") | Some("speex") => {
            if let Some(vorbis_comments) = read_speex_vorbis_comments(path) {
                merge_vorbis_comments(raw_tags, &vorbis_comments);
            }
        }
        _ => {}
    }

    normalize_raw_tag_values(raw_tags);
}

fn collect_raw_tags(tagged_file: &lofty::file::TaggedFile) -> BTreeMap<String, Vec<String>> {
    let mut raw_tags: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for tag in tagged_file.tags() {
        let tag_type = tag.tag_type();
        for item in tag.items() {
            let key = resolve_item_key_name(item.key(), tag_type);
            let values = item_value_strings(item.value());
            if values.is_empty() {
                continue;
            }

            raw_tags.entry(key).or_default().extend(values);
        }
    }

    for values in raw_tags.values_mut() {
        *values = dedupe_preserve_order(values.clone());
    }

    raw_tags
}

fn read_raw_tags_from_path(path: &Path) -> Result<BTreeMap<String, Vec<String>>, LibraryError> {
    let tagged_file = read_from_path(path).map_err(|_| LibraryError::TrackReadFailure)?;
    let mut raw_tags = collect_raw_tags(&tagged_file);
    merge_format_specific_raw_tags(path, &mut raw_tags);
    Ok(raw_tags)
}

fn read_flac_vorbis_comments(path: &Path) -> Option<VorbisComments> {
    let mut file = StdFile::open(path).ok()?;
    let flac = FlacFile::read_from(&mut file, supplemental_parse_options()).ok()?;
    flac.vorbis_comments().cloned()
}

fn read_ogg_vorbis_comments(path: &Path) -> Option<VorbisComments> {
    let mut file = StdFile::open(path).ok()?;
    let vorbis = VorbisFile::read_from(&mut file, supplemental_parse_options()).ok()?;
    Some(vorbis.vorbis_comments().clone())
}

fn read_opus_vorbis_comments(path: &Path) -> Option<VorbisComments> {
    let mut file = StdFile::open(path).ok()?;
    let opus = OpusFile::read_from(&mut file, supplemental_parse_options()).ok()?;
    Some(opus.vorbis_comments().clone())
}

fn read_speex_vorbis_comments(path: &Path) -> Option<VorbisComments> {
    let mut file = StdFile::open(path).ok()?;
    let speex = SpeexFile::read_from(&mut file, supplemental_parse_options()).ok()?;
    Some(speex.vorbis_comments().clone())
}

fn merge_vorbis_comments(
    raw_tags: &mut BTreeMap<String, Vec<String>>,
    vorbis_comments: &VorbisComments,
) {
    for (key, value) in vorbis_comments.items() {
        let values = split_multi_value(value);
        if values.is_empty() {
            continue;
        }

        raw_tags
            .entry(normalize_tag_name(key))
            .or_default()
            .extend(values);
    }
}

fn normalize_raw_tag_values(raw_tags: &mut BTreeMap<String, Vec<String>>) {
    for values in raw_tags.values_mut() {
        *values = dedupe_preserve_order(std::mem::take(values));
    }
}

fn supplemental_parse_options() -> ParseOptions {
    ParseOptions::new()
        .read_properties(false)
        .read_cover_art(false)
}

fn resolve_item_key_name(key: ItemKey, tag_type: TagType) -> String {
    let fallback = format!("{key:?}");

    key.map_key(tag_type)
        .map(normalize_tag_name)
        .unwrap_or_else(|| {
            fallback
                .strip_prefix("Unknown(\"")
                .and_then(|value| value.strip_suffix("\")"))
                .map(normalize_tag_name)
                .unwrap_or_else(|| normalize_tag_name(&fallback))
        })
}

fn item_value_strings(value: &ItemValue) -> Vec<String> {
    let text = match value {
        ItemValue::Text(text) => text.to_string(),
        ItemValue::Locator(locator) => locator.to_string(),
        ItemValue::Binary(_) => return Vec::new(),
    };

    split_multi_value(&text)
}

fn split_multi_value(value: &str) -> Vec<String> {
    let separators = [";", " / ", " | ", "\u{0}"];
    let mut values = vec![value.trim().to_string()];

    for separator in separators {
        values = values
            .into_iter()
            .flat_map(|segment| {
                if segment.contains(separator) {
                    segment
                        .split(separator)
                        .map(|part| part.trim().to_string())
                        .collect::<Vec<_>>()
                } else {
                    vec![segment]
                }
            })
            .collect();
    }

    dedupe_preserve_order(
        values
            .into_iter()
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>(),
    )
}

fn dedupe_preserve_order(values: Vec<String>) -> Vec<String> {
    let mut seen = Vec::new();
    let mut deduped = Vec::new();

    for value in values {
        let normalized = value.to_ascii_lowercase();
        if seen.iter().any(|existing| existing == &normalized) {
            continue;
        }
        seen.push(normalized);
        deduped.push(value);
    }

    deduped
}

fn normalize_tag_name(tag: &str) -> String {
    tag.trim().to_ascii_uppercase()
}

fn normalize_root_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if trimmed.len() <= 3 {
        return trimmed.to_string();
    }

    trimmed.trim_end_matches(['\\', '/']).to_string()
}

fn root_paths_equal(left: &str, right: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        left.to_lowercase() == right.to_lowercase()
    }

    #[cfg(not(target_os = "windows"))]
    {
        left == right
    }
}

fn merge_inventory(
    inventory: &mut HashMap<String, InventoryAccumulator>,
    raw_tags: &BTreeMap<String, Vec<String>>,
) {
    for (tag, values) in raw_tags {
        let entry = inventory.entry(tag.clone()).or_default();
        entry.occurrences += 1;

        for value in values {
            if entry.example_values.len() >= 3 {
                break;
            }
            if !entry
                .example_values
                .iter()
                .any(|existing| existing == value)
            {
                entry.example_values.push(value.clone());
            }
        }
    }
}

fn map_fields(
    raw_tags: &BTreeMap<String, Vec<String>>,
    mappings: &[LibraryFieldMapping],
    catalog_rules: &[CompiledCatalogRule],
) -> BTreeMap<String, Vec<String>> {
    let mut mapped = BTreeMap::new();

    for mapping in mappings {
        let mut resolved_values = Vec::new();

        for source_tag in &mapping.tag_priorities {
            let normalized = normalize_tag_name(source_tag);
            if let Some(values) = raw_tags.get(&normalized) {
                if !values.is_empty() {
                    resolved_values = values.clone();
                    break;
                }
            }
        }

        if resolved_values.is_empty() && mapping.key == "catalog" {
            resolved_values = extract_catalog_numbers(raw_tags, catalog_rules);
        }

        mapped.insert(mapping.key.clone(), dedupe_preserve_order(resolved_values));
    }

    mapped
}

fn extract_catalog_numbers(
    raw_tags: &BTreeMap<String, Vec<String>>,
    catalog_rules: &[CompiledCatalogRule],
) -> Vec<String> {
    let mut values = Vec::new();

    for source in DEFAULT_CATALOG_SOURCE_TAGS {
        let Some(source_values) = raw_tags.get(*source) else {
            continue;
        };

        let mut source_matches = Vec::new();
        for rule in catalog_rules {
            if !catalog_rule_matches_composer(raw_tags, rule) {
                continue;
            }

            for source_value in source_values {
                source_matches.extend(extract_catalog_matches_from_value(
                    source_value,
                    &rule.regex,
                ));
            }
        }

        if !source_matches.is_empty() {
            values.extend(source_matches);
            break;
        }
    }

    dedupe_preserve_order(values)
}

fn extract_catalog_matches_from_value(source_value: &str, regex: &Regex) -> Vec<String> {
    let mut segments = source_value
        .split([':', '：'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    if segments.is_empty() {
        segments.push(source_value);
    }

    for segment in segments.into_iter().rev() {
        let matches = regex
            .find_iter(segment)
            .filter(|matched| !match_is_followed_by_catalog_range(segment, matched.end()))
            .map(|matched| matched.as_str().trim().to_string())
            .collect::<Vec<_>>();

        if !matches.is_empty() {
            return matches;
        }
    }

    Vec::new()
}

fn match_is_followed_by_catalog_range(value: &str, match_end: usize) -> bool {
    let suffix = value[match_end..].trim_start();
    let mut chars = suffix.chars();

    match chars.next() {
        Some('-' | '–' | '—') => {}
        _ => return false,
    }

    chars
        .as_str()
        .trim_start()
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_digit())
}

fn sanitize_catalog_rules(rules: Vec<CatalogRule>) -> Vec<CatalogRule> {
    let sanitized = rules
        .into_iter()
        .filter_map(|rule| {
            let label = normalize_catalog_label(&rule.label);
            if label.is_empty() {
                return None;
            }

            let composers = dedupe_preserve_order(
                rule.composers
                    .into_iter()
                    .map(|composer| composer.trim().to_string())
                    .filter(|composer| !composer.is_empty())
                    .collect(),
            );

            Some(CatalogRule {
                label,
                composers,
                enabled: rule.enabled,
            })
        })
        .collect::<Vec<_>>();

    if sanitized.is_empty() {
        default_catalog_rules()
    } else {
        sanitized
    }
}

fn validate_catalog_rules(rules: &[CatalogRule]) -> Result<(), LibraryError> {
    for rule in rules {
        let label = normalize_catalog_label(&rule.label);
        if label.is_empty() {
            return Err(LibraryError::InvalidCatalogRule {
                label: rule.label.clone(),
                message: "catalog label cannot be empty".into(),
            });
        }

        let _ = build_catalog_label_regex(&label)?;
    }

    Ok(())
}

fn compile_catalog_rules(rules: &[CatalogRule]) -> Vec<CompiledCatalogRule> {
    rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter_map(|rule| {
            let regex = build_catalog_label_regex(&rule.label).ok()?;
            let composer_hints = rule
                .composers
                .iter()
                .map(|composer| composer.trim().to_ascii_lowercase())
                .filter(|composer| !composer.is_empty())
                .collect::<Vec<_>>();

            Some(CompiledCatalogRule {
                regex,
                composer_hints,
            })
        })
        .collect()
}

fn build_catalog_label_regex(label: &str) -> Result<Regex, LibraryError> {
    let normalized = normalize_catalog_label(label);
    if normalized.is_empty() {
        return Err(LibraryError::InvalidCatalogRule {
            label: label.to_string(),
            message: "catalog label cannot be empty".into(),
        });
    }

    let prefix = catalog_label_regex_prefix(&normalized);
    if prefix.is_empty() {
        return Err(LibraryError::InvalidCatalogRule {
            label: normalized,
            message: "catalog label must contain letters or numbers".into(),
        });
    }

    Regex::new(&format!(
        r"(?i)\b{prefix}\s*(?:[IVXLCM]+\s*[:.]\s*)?\d+[A-Za-z]?(?:\s*[:.]\s*[A-Za-z0-9]+)?(?:\s*No\.?\s*\d+)?\b"
    ))
    .map_err(|error| LibraryError::InvalidCatalogRule {
        label: normalized,
        message: error.to_string(),
    })
}

fn catalog_label_regex_prefix(label: &str) -> String {
    if label.eq_ignore_ascii_case("op") || label.eq_ignore_ascii_case("opus") {
        String::from(r"(?:Op\.?|Opus)")
    } else {
        let trimmed = label.trim_end_matches('.').trim();
        if trimmed.is_empty() {
            return String::new();
        }
        format!(r"{}\.?", regex::escape(trimmed))
    }
}

fn normalize_catalog_label(label: &str) -> String {
    let trimmed = label.trim();
    if trimmed.eq_ignore_ascii_case("opus") {
        "Op".into()
    } else {
        trimmed.to_string()
    }
}

fn catalog_rule_matches_composer(
    raw_tags: &BTreeMap<String, Vec<String>>,
    rule: &CompiledCatalogRule,
) -> bool {
    if rule.composer_hints.is_empty() {
        return true;
    }

    let composer_values = COMPOSER_SOURCE_TAGS
        .iter()
        .filter_map(|tag| raw_tags.get(*tag))
        .flat_map(|values| values.iter())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    if composer_values.is_empty() {
        return true;
    }

    composer_values.iter().any(|value| {
        let normalized = value.to_ascii_lowercase();
        rule.composer_hints
            .iter()
            .any(|hint| normalized.contains(hint))
    })
}

fn find_album_art(track_path: &Path, cache_root: &Path) -> Option<PathBuf> {
    if track_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("flac"))
    {
        if let Some(embedded_art) = extract_embedded_flac_art(track_path, cache_root) {
            return Some(embedded_art);
        }
    }

    find_sidecar_album_art(track_path)
}

fn extract_embedded_flac_art(track_path: &Path, cache_root: &Path) -> Option<PathBuf> {
    let mut file = StdFile::open(track_path).ok()?;
    let flac = FlacFile::read_from(&mut file, cover_art_parse_options()).ok()?;
    let picture = choose_embedded_cover(flac.pictures())?;
    let image_bytes = picture.data();
    if image_bytes.is_empty() {
        return None;
    }

    let extension = picture_extension(picture, image_bytes)?;
    let cache_path = embedded_cover_cache_path(track_path, cache_root, extension);

    if std::fs::metadata(&cache_path)
        .ok()
        .is_some_and(|metadata| metadata.len() > 0)
    {
        return Some(cache_path);
    }

    std::fs::create_dir_all(cache_root).ok()?;
    std::fs::write(&cache_path, image_bytes).ok()?;
    Some(cache_path)
}

fn choose_embedded_cover(pictures: &[(Picture, lofty::picture::PictureInformation)]) -> Option<&Picture> {
    pictures
        .iter()
        .filter(|(picture, _)| !picture.data().is_empty())
        .min_by_key(|(picture, _)| embedded_cover_priority(picture.pic_type()))
        .map(|(picture, _)| picture)
}

fn embedded_cover_priority(picture_type: PictureType) -> u8 {
    match picture_type {
        PictureType::CoverFront => 0,
        PictureType::Media => 1,
        PictureType::Illustration => 2,
        PictureType::Artist => 3,
        PictureType::Band => 4,
        PictureType::Other => 5,
        _ => 6,
    }
}

fn picture_extension(picture: &Picture, image_bytes: &[u8]) -> Option<&'static str> {
    match picture.mime_type() {
        Some(MimeType::Jpeg) => Some("jpg"),
        Some(MimeType::Png) => Some("png"),
        Some(MimeType::Tiff) => Some("tif"),
        Some(MimeType::Bmp) => Some("bmp"),
        Some(MimeType::Gif) => Some("gif"),
        Some(MimeType::Unknown(_)) | None | Some(_) => guess_image_extension(image_bytes),
    }
}

fn guess_image_extension(image_bytes: &[u8]) -> Option<&'static str> {
    if image_bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if image_bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if image_bytes.starts_with(b"GIF87a") || image_bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if image_bytes.starts_with(b"BM") {
        Some("bmp")
    } else if image_bytes.starts_with(b"II*\0") || image_bytes.starts_with(b"MM\0*") {
        Some("tif")
    } else {
        None
    }
}

fn embedded_cover_cache_path(track_path: &Path, cache_root: &Path, extension: &str) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    track_path.hash(&mut hasher);
    let cache_key = hasher.finish();
    cache_root.join(format!("embedded-{cache_key:016x}.{extension}"))
}

fn find_sidecar_album_art(track_path: &Path) -> Option<PathBuf> {
    const ART_FILES: &[&str] = &[
        "cover.jpg",
        "folder.jpg",
        "front.jpg",
        "cover.png",
        "folder.png",
        "front.png",
    ];

    let mut candidates = Vec::new();
    if let Some(current_dir) = track_path.parent() {
        candidates.push(current_dir.to_path_buf());

        if let Some(parent_dir) = current_dir.parent() {
            let current_name = current_dir
                .file_name()
                .and_then(|segment| segment.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();

            if current_name.starts_with("disc ")
                || current_name.starts_with("disc_")
                || current_name.starts_with("cd ")
                || current_name.starts_with("cd")
            {
                candidates.push(parent_dir.to_path_buf());
            }
        }
    }

    for directory in candidates {
        for art_name in ART_FILES {
            let candidate = directory.join(art_name);
            if sidecar_art_is_usable(&candidate) {
                return Some(candidate);
            }
        }
    }

    None
}

fn sidecar_art_is_usable(path: &Path) -> bool {
    std::fs::metadata(path)
        .ok()
        .is_some_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

fn cover_art_parse_options() -> ParseOptions {
    ParseOptions::new()
        .read_properties(false)
        .read_cover_art(true)
}

#[cfg(test)]
mod tests {
    use super::{
        compile_catalog_rules, extract_catalog_numbers, find_album_art, scan_library,
        scan_single_track,
    };
    use std::{
        collections::BTreeMap,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use aria_domain::{default_catalog_rules, default_field_mappings, LibraryRoot};
    use tokio::sync::broadcast;

    #[test]
    fn can_scan_sample_library_when_present() {
        let sample_root = PathBuf::from(r"C:\Users\hongswan\Music\Great 50\Great 50");

        if !sample_root.exists() {
            return;
        }

        let (events, _) = broadcast::channel(64);
        let artifacts = scan_library(
            &[LibraryRoot {
                path: sample_root.to_string_lossy().into_owned(),
                label: "Great 50".into(),
            }],
            &default_field_mappings(),
            &default_catalog_rules(),
            &PathBuf::from(r"C:\dev\aria\.cache\album-art"),
            &events,
        )
        .expect("sample library should scan");

        assert!(!artifacts.tracks.is_empty(), "expected scanned tracks");
        assert!(
            artifacts
                .tag_inventory
                .iter()
                .any(|entry| entry.tag == "ALBUM"),
            "expected ALBUM tag to appear"
        );
        assert!(
            artifacts.tracks.iter().any(|track| !track
                .mapped_fields
                .get("title")
                .cloned()
                .unwrap_or_default()
                .is_empty()),
            "expected title mapping for at least one track"
        );
    }

    #[test]
    fn preserves_custom_flac_vorbis_fields_for_sample_track_when_present() {
        let sample_track = PathBuf::from(
            r"C:\Users\hongswan\Music\Great 50\Great 50\Renaud Capuçon - Brahms - Berg  Violin Concertos (2012) [16B-44.1kHz]\05. Violin Concerto To the Memory of an Angel II. Allegro - Adagio.flac",
        );

        if !sample_track.exists() {
            return;
        }

        let track = scan_single_track(
            &sample_track,
            &default_field_mappings(),
            &compile_catalog_rules(&default_catalog_rules()),
            &PathBuf::from(r"C:\dev\aria\.cache\album-art"),
        )
        .expect("sample track should scan");

        assert!(
            track.raw_tags.contains_key("ENSEMBLE"),
            "expected ENSEMBLE raw tag to be preserved"
        );
        assert!(
            !track
                .mapped_fields
                .get("ensemble")
                .cloned()
                .unwrap_or_default()
                .is_empty(),
            "expected ensemble field mapping to resolve"
        );
    }

    #[test]
    fn embedded_flac_art_wins_over_empty_sidecar_when_sample_track_is_present() {
        let sample_track = PathBuf::from(
            r"C:\Users\hongswan\Music\Bach 50\music\Murray Perahia - Bach Keyboard Concertos, Vol. 1 (2001) [16B-44.1kHz]\01. I. Allegro.flac",
        );

        if !sample_track.exists() {
            return;
        }

        let cache_root = PathBuf::from(r"C:\dev\aria\.cache\album-art-tests");
        let album_art = find_album_art(&sample_track, &cache_root)
            .expect("sample track should resolve album art");

        assert!(
            album_art.starts_with(&cache_root),
            "expected embedded FLAC art to be cached before any sidecar fallback"
        );
        assert!(
            std::fs::metadata(&album_art)
                .map(|metadata| metadata.len() > 0)
                .unwrap_or(false),
            "expected cached embedded art to be non-empty"
        );
    }

    #[test]
    fn sidecar_lookup_skips_zero_byte_files() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("aria-sidecar-test-{unique}"));
        let cache_root = temp_root.join("cache");
        std::fs::create_dir_all(&temp_root).expect("temp directory should be created");

        let track_path = temp_root.join("track.mp3");
        std::fs::write(&track_path, b"not audio but good enough for path lookup")
            .expect("track placeholder should be written");
        std::fs::write(temp_root.join("cover.jpg"), [])
            .expect("empty cover placeholder should be written");
        std::fs::write(
            temp_root.join("front.png"),
            [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0],
        )
        .expect("fallback art should be written");

        let album_art = find_album_art(&track_path, &cache_root)
            .expect("expected non-empty fallback sidecar to be selected");

        assert_eq!(album_art, temp_root.join("front.png"));

        std::fs::remove_dir_all(&temp_root).ok();
    }

    #[test]
    fn composer_aware_catalog_rules_extract_wab_for_bruckner() {
        let rules = compile_catalog_rules(&default_catalog_rules());
        let raw_tags = BTreeMap::from([
            ("COMPOSER".into(), vec!["Anton Bruckner".into()]),
            (
                "TITLE".into(),
                vec!["Symphony No. 4 in E-flat major, WAB 104".into()],
            ),
        ]);

        let values = extract_catalog_numbers(&raw_tags, &rules);

        assert_eq!(values, vec!["WAB 104"]);
    }

    #[test]
    fn composer_aware_catalog_rules_skip_wab_when_composer_does_not_match() {
        let rules = compile_catalog_rules(&default_catalog_rules());
        let raw_tags = BTreeMap::from([
            ("COMPOSER".into(), vec!["Wolfgang Amadeus Mozart".into()]),
            ("TITLE".into(), vec!["Symphony in C major, WAB 104".into()]),
        ]);

        let values = extract_catalog_numbers(&raw_tags, &rules);

        assert!(values.is_empty());
    }

    #[test]
    fn composer_aware_catalog_rules_allow_explicit_label_when_composer_is_missing() {
        let rules = compile_catalog_rules(&default_catalog_rules());
        let raw_tags = BTreeMap::from([(
            "TITLE".into(),
            vec![
                "Messiah, HWV 56, Pt. 1: No. 20, Aria. He Shall Feed His Flock Like a Shepherd (Alto/Soprano)"
                    .into(),
            ],
        )]);

        let values = extract_catalog_numbers(&raw_tags, &rules);

        assert_eq!(values, vec!["HWV 56"]);
    }

    #[test]
    fn catalog_rules_respect_source_tag_priority_before_falling_back_to_album() {
        let rules = compile_catalog_rules(&default_catalog_rules());
        let raw_tags = BTreeMap::from([
            ("COMPOSER".into(), vec!["Johann Sebastian Bach".into()]),
            (
                "TITLE".into(),
                vec!["Keyboard Partita No. 2 in C Minor, BWV 826: I. Sinfonia".into()],
            ),
            ("ALBUM".into(), vec!["Bach: 6 Partitas, BWV 825-830".into()]),
        ]);

        let values = extract_catalog_numbers(&raw_tags, &rules);

        assert_eq!(values, vec!["BWV 826"]);
    }

    #[test]
    fn catalog_rules_prefer_last_matching_title_segment_for_track_catalogs() {
        let rules = compile_catalog_rules(&default_catalog_rules());
        let raw_tags = BTreeMap::from([
            ("COMPOSER".into(), vec!["Johann Sebastian Bach".into()]),
            (
                "TITLE".into(),
                vec![
                    "Das Wohltemperierte Klavier: Book 1, BWV 846-869: Präludium Es-Dur, BWV 852"
                        .into(),
                ],
            ),
        ]);

        let values = extract_catalog_numbers(&raw_tags, &rules);

        assert_eq!(values, vec!["BWV 852"]);
    }

    #[test]
    fn catch_all_op_rule_applies_when_no_composer_specific_label_matches() {
        let rules = compile_catalog_rules(&default_catalog_rules());
        let raw_tags = BTreeMap::from([
            ("COMPOSER".into(), vec!["Ludwig van Beethoven".into()]),
            (
                "TITLE".into(),
                vec!["Piano Sonata No. 23 in F minor, Op. 57".into()],
            ),
        ]);

        let values = extract_catalog_numbers(&raw_tags, &rules);

        assert_eq!(values, vec!["Op. 57"]);
    }

}
