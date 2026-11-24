use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fs::File as StdFile,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::Arc,
};

use aria_domain::{
    canonical_field_mapping_format, default_catalog_rules, AppEvent, AudioPropertiesSnapshot,
    CatalogRule, FieldExportRequest, LibraryEvent, LibraryFieldMapping, LibraryRoot,
    LibrarySnapshot, ScanProgress, ScannedTrack, TagInventoryEntry, TrackTagEditRequest,
    TrackTagEditUpdate,
};
use chrono::Utc;
use lofty::{
    aac::AacFile,
    config::{ParseOptions, WriteOptions},
    file::{AudioFile, FileType, TaggedFileExt},
    flac::FlacFile,
    id3::v2::Id3v2Tag,
    iff::{aiff::AiffFile, wav::WavFile},
    mp4::{Atom, AtomData, AtomIdent, Ilst, Mp4File},
    mpeg::MpegFile,
    ogg::{OggPictureStorage, OpusFile, SpeexFile, VorbisComments, VorbisFile},
    picture::{MimeType, Picture, PictureType},
    read_from_path,
    tag::{ItemKey, ItemValue, MergeTag, SplitTag, TagExt, TagItem, TagType},
};
use regex::Regex;
use tokio::{
    sync::{broadcast, Mutex, RwLock},
    task::spawn_blocking,
};
use walkdir::WalkDir;

use crate::LibraryError;

#[derive(Clone, Default)]
pub struct LibraryService {
    state: Arc<RwLock<LibrarySnapshot>>,
    write_lock: Arc<Mutex<()>>,
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
    is_single_letter_label: bool,
}

const CATALOG_TITLE_SOURCE_TAGS: &[&str] = &["TITLE", "©NAM", "TIT2", "INAM", "NAME"];
const CATALOG_WORK_SOURCE_TAGS: &[&str] = &["WORK", "©WRK", "TIT1", "----:com.apple.iTunes:WORK"];
const CATALOG_ALBUM_SOURCE_TAGS: &[&str] = &["ALBUM", "©ALB", "TALB", "IPRD"];
const CATALOG_SOURCE_TAG_TIERS: &[&[&str]] = &[
    CATALOG_TITLE_SOURCE_TAGS,
    CATALOG_WORK_SOURCE_TAGS,
    CATALOG_ALBUM_SOURCE_TAGS,
];
const COMPOSER_SOURCE_TAGS: &[&str] = &[
    "COMPOSER",
    "©WRT",
    "TCOM",
    "IMUS",
    "WORKCOMPOSER",
    "COMPOSERSORT",
    "----:com.apple.iTunes:COMPOSER",
];

fn remap_tracks_preserving_synthetic_fields(
    tracks: &mut [ScannedTrack],
    mappings: &[LibraryFieldMapping],
    catalog_rules: &[CompiledCatalogRule],
) {
    for track in tracks {
        if track.raw_tags.is_empty() {
            continue;
        }
        let album_path = track.mapped_fields.get("album_path").cloned();
        track.mapped_fields =
            map_fields(&track.raw_tags, mappings, &track.audio.format, catalog_rules);
        if let Some(ap) = album_path {
            track.mapped_fields.insert("album_path".to_string(), ap);
        }
    }
}

impl LibraryService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_snapshot(mut snapshot: LibrarySnapshot) -> Self {
        snapshot.is_scanning = false;
        if snapshot.catalog_rules.is_empty() {
            snapshot.catalog_rules = default_catalog_rules();
        }

        let mappings = snapshot.field_mappings.clone();
        let catalog_rules = compile_catalog_rules(&snapshot.catalog_rules);
        remap_tracks_preserving_synthetic_fields(&mut snapshot.tracks, &mappings, &catalog_rules);

        Self {
            state: Arc::new(RwLock::new(snapshot)),
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn snapshot_now(&self) -> LibrarySnapshot {
        self.state
            .try_read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
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

    pub async fn merge_remote_tracks(&self, remote_tracks: Vec<ScannedTrack>) -> LibrarySnapshot {
        let mut state = self.state.write().await;
        let mut track_indices: HashMap<String, usize> = state
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id.clone(), i))
            .collect();

        for new_track in remote_tracks {
            if let Some(&pos) = track_indices.get(&new_track.id) {
                if state.tracks[pos].path.starts_with("remote://") {
                    state.tracks[pos] = new_track;
                }
            } else {
                let new_pos = state.tracks.len();
                track_indices.insert(new_track.id.clone(), new_pos);
                state.tracks.push(new_track);
            }
        }
        state.tag_inventory = build_tag_inventory(&state.tracks);
        state.indexed_files = state.tracks.len() as u64;
        state.clone()
    }

    pub async fn remove_remote_tracks_for_target(&self, target_id: &str) -> LibrarySnapshot {
        let prefix = format!("remote://{target_id}/");
        let mut state = self.state.write().await;
        state.tracks.retain(|t| !t.path.starts_with(&prefix));
        state.tag_inventory = build_tag_inventory(&state.tracks);
        state.indexed_files = state.tracks.len() as u64;
        state.clone()
    }

    pub async fn prune_remote_tracks_not_in_folders(
        &self,
        target_id: &str,
        valid_folders: &HashSet<String>,
    ) -> (LibrarySnapshot, Vec<String>) {
        let prefix = format!("remote://{target_id}/");
        let mut state = self.state.write().await;
        let mut removed_ids = Vec::new();
        state.tracks.retain(|t| {
            if let Some(rel) = t.path.strip_prefix(&prefix) {
                if let Some((folder, _)) = rel.rsplit_once('/') {
                    if valid_folders.contains(folder) {
                        return true;
                    }
                }
                removed_ids.push(t.id.clone());
                false
            } else {
                true
            }
        });
        if !removed_ids.is_empty() {
            state.tag_inventory = build_tag_inventory(&state.tracks);
            state.indexed_files = state.tracks.len() as u64;
        }
        (state.clone(), removed_ids)
    }

    pub async fn update_track_path(&self, track_id: &str, new_path: &str) {
        let mut state = self.state.write().await;
        if let Some(track) = state.tracks.iter_mut().find(|t| t.id == track_id) {
            track.path = new_path.to_string();
        }
    }

    pub async fn set_field_mappings(&self, mappings: Vec<LibraryFieldMapping>) -> LibrarySnapshot {
        let mut state = self.state.write().await;
        state.field_mappings = mappings
            .into_iter()
            .filter(|mapping| !mapping.key.trim().is_empty() && !mapping.label.trim().is_empty())
            .map(|mut mapping| {
                mapping.format = canonical_field_mapping_format(&mapping.format);
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
        remap_tracks_preserving_synthetic_fields(&mut state.tracks, &mappings, &catalog_rules);

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
        remap_tracks_preserving_synthetic_fields(&mut state.tracks, &mappings, &catalog_rules);

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
                        let has_remote = state.tracks.iter().any(|t| t.path.starts_with("remote://"));
                        let (final_tracks, final_inventory, final_count) = if has_remote {
                            let mut combined_tracks = artifacts.tracks;
                            for track in &state.tracks {
                                if track.path.starts_with("remote://")
                                    && !combined_tracks.iter().any(|t| t.id == track.id)
                                {
                                    combined_tracks.push(track.clone());
                                }
                            }
                            let count = combined_tracks.len() as u64;
                            let inv = build_tag_inventory(&combined_tracks);
                            (combined_tracks, inv, count)
                        } else {
                            (artifacts.tracks, artifacts.tag_inventory, artifacts.indexed_files)
                        };

                        state.is_scanning = false;
                        state.indexed_files = final_count;
                        state.last_scan_at = Some(Utc::now().to_rfc3339());
                        state.tag_inventory = final_inventory;
                        state.tracks = final_tracks;
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

    pub async fn export_field_to_tag(
        &self,
        request: FieldExportRequest,
    ) -> Result<LibrarySnapshot, LibraryError> {
        let track_paths = dedupe_preserve_order(
            request
                .track_paths
                .into_iter()
                .map(|path| path.trim().to_string())
                .filter(|path| !path.is_empty())
                .collect(),
        );
        let field_key = request.field_key.trim().to_string();
        let tag_name = request.tag_name.trim().to_string();

        if track_paths.is_empty() {
            return Err(LibraryError::EmptyFieldExportSelection);
        }

        if field_key.is_empty() {
            return Err(LibraryError::InvalidFieldExportField);
        }

        if tag_name.is_empty() {
            return Err(LibraryError::InvalidFieldExportTag);
        }

        let _write_guard = self.write_lock.lock().await;

        let snapshot = self.state.read().await.clone();
        if snapshot.is_scanning {
            return Err(LibraryError::AlreadyScanning);
        }

        let field_exists = snapshot
            .field_mappings
            .iter()
            .any(|mapping| mapping.key == field_key)
            || snapshot
                .tracks
                .iter()
                .any(|track| track.mapped_fields.contains_key(&field_key));

        if !field_exists {
            return Err(LibraryError::InvalidFieldExportField);
        }

        let track_lookup = snapshot
            .tracks
            .iter()
            .cloned()
            .map(|track| (track.path.clone(), track))
            .collect::<HashMap<_, _>>();
        let mappings = snapshot.field_mappings.clone();
        let compiled_catalog_rules = compile_catalog_rules(&snapshot.catalog_rules);
        let cache_root = album_art_cache_root();

        let (updated_tracks, batch_error) = spawn_blocking(move || {
            export_field_to_tag_from_tracks(
                &track_lookup,
                &track_paths,
                &field_key,
                &tag_name,
                &mappings,
                &compiled_catalog_rules,
                &cache_root,
            )
        })
        .await
        .map_err(|_| LibraryError::FieldExportRefreshFailure)?;

        if !updated_tracks.is_empty() {
            let updated_track_lookup = updated_tracks
                .into_iter()
                .map(|track| (track.path.clone(), track))
                .collect::<HashMap<_, _>>();

            let mut state = self.state.write().await;
            for track in &mut state.tracks {
                if let Some(updated) = updated_track_lookup.get(&track.path) {
                    *track = updated.clone();
                }
            }
            state.tag_inventory = build_tag_inventory(&state.tracks);
        }

        if let Some(err) = batch_error {
            return Err(err);
        }

        Ok(self.state.read().await.clone())
    }

    pub async fn edit_track_tags(
        &self,
        request: TrackTagEditRequest,
    ) -> Result<LibrarySnapshot, LibraryError> {
        let track_paths = dedupe_preserve_order(
            request
                .track_paths
                .into_iter()
                .map(|path| path.trim().to_string())
                .filter(|path| !path.is_empty())
                .collect(),
        );

        if track_paths.is_empty() {
            return Err(LibraryError::EmptyTrackTagEditSelection);
        }

        let updates = sanitize_track_tag_edit_updates(request.updates)?;
        let _write_guard = self.write_lock.lock().await;
        let snapshot = self.state.read().await.clone();
        if updates.is_empty() {
            return Ok(snapshot);
        }

        if snapshot.is_scanning {
            return Err(LibraryError::AlreadyScanning);
        }

        let track_lookup = snapshot
            .tracks
            .iter()
            .cloned()
            .map(|track| (track.path.clone(), track))
            .collect::<HashMap<_, _>>();
        let mappings = snapshot.field_mappings.clone();
        let compiled_catalog_rules = compile_catalog_rules(&snapshot.catalog_rules);
        let cache_root = album_art_cache_root();

        let (updated_tracks, batch_error) = spawn_blocking(move || {
            edit_track_tags_from_tracks(
                &track_lookup,
                &track_paths,
                &updates,
                &mappings,
                &compiled_catalog_rules,
                &cache_root,
            )
        })
        .await
        .map_err(|_| LibraryError::FieldExportRefreshFailure)?;

        if !updated_tracks.is_empty() {
            let updated_track_lookup = updated_tracks
                .into_iter()
                .map(|track| (track.path.clone(), track))
                .collect::<HashMap<_, _>>();

            let mut state = self.state.write().await;
            for track in &mut state.tracks {
                if let Some(updated) = updated_track_lookup.get(&track.path) {
                    *track = updated.clone();
                }
            }
            state.tag_inventory = build_tag_inventory(&state.tracks);
        }

        if let Some(err) = batch_error {
            return Err(err);
        }

        Ok(self.state.read().await.clone())
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

    std::fs::create_dir_all(cache_root).ok();

    for (index, path) in audio_files.iter().enumerate() {
        match scan_single_track(path, mappings, &compiled_catalog_rules, cache_root) {
            Ok(track) => {
                tracks.push(track);
            }
            Err(_) => {
                failed_files += 1;
            }
        }

        let processed_files = (index + 1) as u64;
        if processed_files == discovered_files || processed_files.is_multiple_of(25) {
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

    let tag_inventory = build_tag_inventory(&tracks);

    Ok(ScanArtifacts {
        indexed_files: tracks.len() as u64,
        tracks,
        tag_inventory,
    })
}

pub fn album_art_cache_root() -> PathBuf {
    #[cfg(target_os = "android")]
    let base_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/data/data/com.aria.player"))
        .join("cache");

    #[cfg(not(target_os = "android"))]
    let base_dir = dirs::data_local_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(std::env::temp_dir);

    base_dir.join("Aria").join("album-art")
}

fn should_skip_entry(entry: &walkdir::DirEntry) -> bool {
    let name = entry.file_name().to_string_lossy();
    if name.starts_with('.') {
        return true;
    }

    let name_lower = name.to_lowercase();
    if name_lower == "$recycle.bin" || name_lower == "system volume information" {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if let Ok(metadata) = entry.metadata() {
            let attrs = metadata.file_attributes();
            // 0x2 = FILE_ATTRIBUTE_HIDDEN, 0x4 = FILE_ATTRIBUTE_SYSTEM
            if (attrs & 0x2) != 0 || (attrs & 0x4) != 0 {
                return true;
            }
        }
    }

    false
}

fn discover_audio_files(roots: &[LibraryRoot]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();

    for root in roots {
        let walker = WalkDir::new(&root.path)
            .follow_links(true)
            .into_iter()
            .filter_entry(|entry| {
                if entry.depth() == 0 {
                    return true;
                }
                !should_skip_entry(entry)
            })
            .filter_map(Result::ok);

        for entry in walker {
            let path = entry.path();
            if entry.file_type().is_file() && is_supported_audio_file(path) {
                let dedupe_key = canonical_path_dedupe_key(path);
                if seen.insert(dedupe_key) {
                    files.push(path.to_path_buf());
                }
            }
        }
    }

    files
}

fn canonical_path_dedupe_key(path: &Path) -> String {
    let raw = path.to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        raw.replace('/', "\\").to_lowercase()
    }
    #[cfg(not(target_os = "windows"))]
    {
        raw.into_owned()
    }
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
    let mapped_fields = map_fields(
        &raw_tags,
        mappings,
        &path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_uppercase())
            .unwrap_or_else(|| "UNKNOWN".into()),
        catalog_rules,
    );
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
        Some("m4a") | Some("mp4") => {
            if let Some(ilst) = read_mp4_ilst(path) {
                merge_mp4_ilst_tags(raw_tags, &ilst);
            }
        }
        _ => {}
    }

    normalize_raw_tag_values(raw_tags);
}

fn collect_raw_tags(tagged_file: &lofty::file::TaggedFile) -> BTreeMap<String, Vec<String>> {
    let mut raw_tags: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for tag in tagged_file.tags() {
        collect_raw_tags_from_tag(tag, &mut raw_tags);
    }

    normalize_raw_tag_values(&mut raw_tags);
    raw_tags
}

fn collect_raw_tags_from_tag(tag: &lofty::tag::Tag, raw_tags: &mut BTreeMap<String, Vec<String>>) {
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

fn read_raw_tags_from_path(path: &Path) -> Result<BTreeMap<String, Vec<String>>, LibraryError> {
    let tagged_file = read_from_path(path).map_err(|_| LibraryError::TrackReadFailure)?;
    let mut raw_tags = collect_raw_tags(&tagged_file);
    merge_format_specific_raw_tags(path, &mut raw_tags);
    Ok(raw_tags)
}

fn sanitize_track_tag_edit_updates(
    updates: Vec<TrackTagEditUpdate>,
) -> Result<Vec<TrackTagEditUpdate>, LibraryError> {
    let mut sanitized_updates: Vec<TrackTagEditUpdate> = Vec::new();

    for update in updates {
        let tag_name = normalize_tag_name(&update.tag_name);
        if tag_name.is_empty() {
            return Err(LibraryError::InvalidTrackTagEdit);
        }

        let values = dedupe_preserve_order(
            update
                .values
                .into_iter()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect(),
        );

        if let Some(existing) = sanitized_updates
            .iter_mut()
            .find(|existing| existing.tag_name == tag_name)
        {
            existing.values = values;
            continue;
        }

        sanitized_updates.push(TrackTagEditUpdate { tag_name, values });
    }

    Ok(sanitized_updates)
}

fn export_field_to_tag_from_tracks(
    track_lookup: &HashMap<String, ScannedTrack>,
    track_paths: &[String],
    field_key: &str,
    tag_name: &str,
    mappings: &[LibraryFieldMapping],
    catalog_rules: &[CompiledCatalogRule],
    cache_root: &Path,
) -> (Vec<ScannedTrack>, Option<LibraryError>) {
    let mut updated_tracks = Vec::with_capacity(track_paths.len());

    for track_path in track_paths {
        let Some(track) = track_lookup.get(track_path) else {
            return (
                updated_tracks,
                Some(LibraryError::FieldExportTrackNotFound {
                    path: track_path.clone(),
                }),
            );
        };

        let values = track.mapped_fields.get(field_key).cloned().unwrap_or_default();
        let path = PathBuf::from(track_path);

        if let Err(err) = write_tag_updates_to_path(&path, &[(tag_name, values.as_slice())]) {
            return (updated_tracks, Some(err));
        }

        match scan_single_track(&path, mappings, catalog_rules, cache_root) {
            Ok(refreshed_track) => updated_tracks.push(refreshed_track),
            Err(_) => {
                return (
                    updated_tracks,
                    Some(LibraryError::FieldExportTrackRefreshFailure {
                        path: track_path.clone(),
                    }),
                );
            }
        }
    }

    (updated_tracks, None)
}

fn edit_track_tags_from_tracks(
    track_lookup: &HashMap<String, ScannedTrack>,
    track_paths: &[String],
    updates: &[TrackTagEditUpdate],
    mappings: &[LibraryFieldMapping],
    catalog_rules: &[CompiledCatalogRule],
    cache_root: &Path,
) -> (Vec<ScannedTrack>, Option<LibraryError>) {
    let mut updated_tracks = Vec::with_capacity(track_paths.len());
    let update_pairs: Vec<(&str, &[String])> = updates
        .iter()
        .map(|u| (u.tag_name.as_str(), u.values.as_slice()))
        .collect();

    for track_path in track_paths {
        if !track_lookup.contains_key(track_path) {
            return (
                updated_tracks,
                Some(LibraryError::FieldExportTrackNotFound {
                    path: track_path.clone(),
                }),
            );
        }

        let path = PathBuf::from(track_path);
        if let Err(err) = write_tag_updates_to_path(&path, &update_pairs) {
            return (updated_tracks, Some(err));
        }

        match scan_single_track(&path, mappings, catalog_rules, cache_root) {
            Ok(refreshed_track) => updated_tracks.push(refreshed_track),
            Err(_) => {
                return (
                    updated_tracks,
                    Some(LibraryError::FieldExportTrackRefreshFailure {
                        path: track_path.clone(),
                    }),
                );
            }
        }
    }

    (updated_tracks, None)
}

fn write_tag_updates_to_path(
    path: &Path,
    updates: &[(&str, &[String])],
) -> Result<(), LibraryError> {
    let Some(file_type) = path.extension().and_then(FileType::from_ext) else {
        return Err(LibraryError::FieldExportWriteFailure {
            path: path.to_string_lossy().into_owned(),
            message: "unsupported audio format".into(),
        });
    };

    let write_err = |message: String| LibraryError::FieldExportWriteFailure {
        path: path.to_string_lossy().into_owned(),
        message,
    };

    match file_type {
        FileType::Aac => {
            let mut audio = {
                let mut file = StdFile::open(path).map_err(|e| write_err(e.to_string()))?;
                AacFile::read_from(&mut file, tag_write_parse_options())
                    .map_err(|e| write_err(e.to_string()))?
            };
            for (tag_name, values) in updates {
                apply_id3v2_export(&mut audio, values, tag_name);
            }
            audio
                .save_to_path(path, WriteOptions::default())
                .map_err(|e| write_err(e.to_string()))?;
        }
        FileType::Aiff => {
            let mut audio = {
                let mut file = StdFile::open(path).map_err(|e| write_err(e.to_string()))?;
                AiffFile::read_from(&mut file, tag_write_parse_options())
                    .map_err(|e| write_err(e.to_string()))?
            };
            for (tag_name, values) in updates {
                apply_id3v2_export(&mut audio, values, tag_name);
            }
            audio
                .save_to_path(path, WriteOptions::default())
                .map_err(|e| write_err(e.to_string()))?;
        }
        FileType::Flac => {
            let mut audio = {
                let mut file = StdFile::open(path).map_err(|e| write_err(e.to_string()))?;
                FlacFile::read_from(&mut file, tag_write_parse_options())
                    .map_err(|e| write_err(e.to_string()))?
            };
            for (tag_name, values) in updates {
                apply_vorbis_export(&mut audio, values, tag_name);
            }
            audio
                .save_to_path(path, WriteOptions::default())
                .map_err(|e| write_err(e.to_string()))?;
        }
        FileType::Mpeg => {
            let mut audio = {
                let mut file = StdFile::open(path).map_err(|e| write_err(e.to_string()))?;
                MpegFile::read_from(&mut file, tag_write_parse_options())
                    .map_err(|e| write_err(e.to_string()))?
            };
            for (tag_name, values) in updates {
                apply_id3v2_export(&mut audio, values, tag_name);
            }
            audio
                .save_to_path(path, WriteOptions::default())
                .map_err(|e| write_err(e.to_string()))?;
        }
        FileType::Mp4 => {
            let mut audio = {
                let mut file = StdFile::open(path).map_err(|e| write_err(e.to_string()))?;
                Mp4File::read_from(&mut file, tag_write_parse_options())
                    .map_err(|e| write_err(e.to_string()))?
            };
            for (tag_name, values) in updates {
                apply_ilst_export(&mut audio, values, tag_name);
            }
            audio
                .save_to_path(path, WriteOptions::default())
                .map_err(|e| write_err(e.to_string()))?;
        }
        FileType::Opus => {
            let mut audio = {
                let mut file = StdFile::open(path).map_err(|e| write_err(e.to_string()))?;
                OpusFile::read_from(&mut file, tag_write_parse_options())
                    .map_err(|e| write_err(e.to_string()))?
            };
            for (tag_name, values) in updates {
                apply_vorbis_export(&mut audio, values, tag_name);
            }
            audio
                .save_to_path(path, WriteOptions::default())
                .map_err(|e| write_err(e.to_string()))?;
        }
        FileType::Vorbis => {
            let mut audio = {
                let mut file = StdFile::open(path).map_err(|e| write_err(e.to_string()))?;
                VorbisFile::read_from(&mut file, tag_write_parse_options())
                    .map_err(|e| write_err(e.to_string()))?
            };
            for (tag_name, values) in updates {
                apply_vorbis_export(&mut audio, values, tag_name);
            }
            audio
                .save_to_path(path, WriteOptions::default())
                .map_err(|e| write_err(e.to_string()))?;
        }
        FileType::Speex => {
            let mut audio = {
                let mut file = StdFile::open(path).map_err(|e| write_err(e.to_string()))?;
                SpeexFile::read_from(&mut file, tag_write_parse_options())
                    .map_err(|e| write_err(e.to_string()))?
            };
            for (tag_name, values) in updates {
                apply_vorbis_export(&mut audio, values, tag_name);
            }
            audio
                .save_to_path(path, WriteOptions::default())
                .map_err(|e| write_err(e.to_string()))?;
        }
        FileType::Wav => {
            let mut audio = {
                let mut file = StdFile::open(path).map_err(|e| write_err(e.to_string()))?;
                WavFile::read_from(&mut file, tag_write_parse_options())
                    .map_err(|e| write_err(e.to_string()))?
            };
            for (tag_name, values) in updates {
                apply_id3v2_export(&mut audio, values, tag_name);
            }
            audio
                .save_to_path(path, WriteOptions::default())
                .map_err(|e| write_err(e.to_string()))?;
        }
        _ => {
            return Err(write_err("unsupported audio format".into()));
        }
    }

    Ok(())
}

fn apply_id3v2_export<T>(file: &mut T, values: &[String], tag_name: &str)
where
    T: Id3v2TagContainer,
{
    let should_remove = if let Some(tag) = file.id3v2_tag_mut() {
        write_values_to_id3v2_tag(tag, tag_name, values);
        tag.is_empty()
    } else {
        false
    };

    if should_remove {
        file.remove_id3v2_tag();
        return;
    }

    if file.id3v2_tag_mut().is_some() || values.is_empty() {
        return;
    }

    let mut tag = Id3v2Tag::new();
    write_values_to_id3v2_tag(&mut tag, tag_name, values);
    if !tag.is_empty() {
        file.set_id3v2_tag(tag);
    }
}

fn apply_vorbis_export<T>(file: &mut T, values: &[String], tag_name: &str)
where
    T: VorbisTagContainer,
{
    let should_remove = if let Some(tag) = file.vorbis_tag_mut() {
        write_values_to_vorbis_tag(tag, tag_name, values);
        tag.is_empty()
    } else {
        false
    };

    if should_remove {
        file.remove_vorbis_tag();
        return;
    }

    if file.vorbis_tag_mut().is_some() || values.is_empty() {
        return;
    }

    let mut tag = VorbisComments::new();
    write_values_to_vorbis_tag(&mut tag, tag_name, values);
    if !tag.is_empty() {
        file.set_vorbis_tag(tag);
    }
}

fn apply_ilst_export<T>(file: &mut T, values: &[String], tag_name: &str)
where
    T: IlstTagContainer,
{
    let should_remove = if let Some(tag) = file.ilst_tag_mut() {
        write_values_to_ilst_tag(tag, tag_name, values);
        tag.is_empty()
    } else {
        false
    };

    if should_remove {
        file.remove_ilst_tag();
        return;
    }

    if file.ilst_tag_mut().is_some() || values.is_empty() {
        return;
    }

    let mut tag = Ilst::new();
    write_values_to_ilst_tag(&mut tag, tag_name, values);
    if !tag.is_empty() {
        file.set_ilst_tag(tag);
    }
}

fn write_values_to_id3v2_tag(tag: &mut Id3v2Tag, tag_name: &str, values: &[String]) {
    if let Some(item_key) = ItemKey::from_key(TagType::Id3v2, tag_name) {
        let (remainder, mut generic_tag) = std::mem::take(tag).split_tag();
        apply_values_to_generic_tag(&mut generic_tag, item_key, values);
        *tag = remainder.merge_tag(generic_tag);
        return;
    }

    tag.remove_user_text(tag_name);
    if !values.is_empty() {
        tag.insert_user_text(tag_name.to_string(), join_single_slot_values(values));
    }
}

fn write_values_to_vorbis_tag(tag: &mut VorbisComments, tag_name: &str, values: &[String]) {
    let key = tag_name.to_string();
    let _ = tag.remove(&key).count();
    for value in values {
        tag.push(key.clone(), value.clone());
    }
}

fn write_values_to_ilst_tag(tag: &mut Ilst, tag_name: &str, values: &[String]) {
    if let Some(item_key) = ItemKey::from_key(TagType::Mp4Ilst, tag_name) {
        let (remainder, mut generic_tag) = std::mem::take(tag).split_tag();
        apply_values_to_generic_tag(&mut generic_tag, item_key, values);
        *tag = remainder.merge_tag(generic_tag);
        return;
    }

    let ident = parse_mp4_freeform_ident(tag_name);
    // Remove any existing freeform atoms matching (mean, name) case-insensitively
    // so legacy uppercased `COM.APPLE.ITUNES` atoms do not remain as duplicates.
    if let AtomIdent::Freeform { ref mean, ref name } = ident {
        tag.retain(|atom| {
            !matches!(
                atom.ident(),
                AtomIdent::Freeform {
                    mean: existing_mean,
                    name: existing_name,
                } if existing_mean.eq_ignore_ascii_case(mean)
                    && existing_name.eq_ignore_ascii_case(name)
            )
        });
    }
    let _ = tag.remove(&ident);

    if let Some((first_value, remaining_values)) = values.split_first() {
        tag.replace_atom(Atom::new(ident.clone(), AtomData::UTF8(first_value.clone())));
        for value in remaining_values {
            tag.insert(Atom::new(ident.clone(), AtomData::UTF8(value.clone())));
        }
    }
}

fn apply_values_to_generic_tag(
    tag: &mut lofty::tag::Tag,
    item_key: ItemKey,
    values: &[String],
) {
    tag.remove_key(item_key);
    for value in values {
        tag.push(TagItem::new(item_key, ItemValue::Text(value.clone())));
    }
}

fn join_single_slot_values(values: &[String]) -> String {
    values.join("; ")
}

fn parse_mp4_freeform_ident(tag_name: &str) -> AtomIdent<'static> {
    if let Some(remainder) = tag_name.strip_prefix("----:") {
        let mut parts = remainder.splitn(2, ':');
        let mean = parts.next().unwrap_or_default().trim();
        let name = parts.next().unwrap_or_default().trim();

        if !mean.is_empty() && !name.is_empty() {
            let canonical_mean = if mean.eq_ignore_ascii_case("com.apple.iTunes") {
                "com.apple.iTunes"
            } else {
                mean
            };
            return AtomIdent::Freeform {
                mean: Cow::Owned(canonical_mean.to_string()),
                name: Cow::Owned(name.to_string()),
            };
        }
    }

    AtomIdent::Freeform {
        mean: Cow::Owned("com.apple.iTunes".to_string()),
        name: Cow::Owned(tag_name.to_string()),
    }
}

fn tag_write_parse_options() -> ParseOptions {
    ParseOptions::new()
        .read_properties(false)
        .read_cover_art(true)
}

trait Id3v2TagContainer {
    fn id3v2_tag_mut(&mut self) -> Option<&mut Id3v2Tag>;
    fn set_id3v2_tag(&mut self, tag: Id3v2Tag);
    fn remove_id3v2_tag(&mut self);
}

impl Id3v2TagContainer for AacFile {
    fn id3v2_tag_mut(&mut self) -> Option<&mut Id3v2Tag> {
        self.id3v2_mut()
    }

    fn set_id3v2_tag(&mut self, tag: Id3v2Tag) {
        let _ = self.set_id3v2(tag);
    }

    fn remove_id3v2_tag(&mut self) {
        let _ = self.remove_id3v2();
    }
}

impl Id3v2TagContainer for AiffFile {
    fn id3v2_tag_mut(&mut self) -> Option<&mut Id3v2Tag> {
        self.id3v2_mut()
    }

    fn set_id3v2_tag(&mut self, tag: Id3v2Tag) {
        let _ = self.set_id3v2(tag);
    }

    fn remove_id3v2_tag(&mut self) {
        let _ = self.remove_id3v2();
    }
}

impl Id3v2TagContainer for MpegFile {
    fn id3v2_tag_mut(&mut self) -> Option<&mut Id3v2Tag> {
        self.id3v2_mut()
    }

    fn set_id3v2_tag(&mut self, tag: Id3v2Tag) {
        let _ = self.set_id3v2(tag);
    }

    fn remove_id3v2_tag(&mut self) {
        let _ = self.remove_id3v2();
    }
}

impl Id3v2TagContainer for WavFile {
    fn id3v2_tag_mut(&mut self) -> Option<&mut Id3v2Tag> {
        self.id3v2_mut()
    }

    fn set_id3v2_tag(&mut self, tag: Id3v2Tag) {
        let _ = self.set_id3v2(tag);
    }

    fn remove_id3v2_tag(&mut self) {
        let _ = self.remove_id3v2();
    }
}

trait VorbisTagContainer {
    fn vorbis_tag_mut(&mut self) -> Option<&mut VorbisComments>;
    fn set_vorbis_tag(&mut self, tag: VorbisComments);
    fn remove_vorbis_tag(&mut self);
}

impl VorbisTagContainer for FlacFile {
    fn vorbis_tag_mut(&mut self) -> Option<&mut VorbisComments> {
        self.vorbis_comments_mut()
    }

    fn set_vorbis_tag(&mut self, tag: VorbisComments) {
        let _ = self.set_vorbis_comments(tag);
    }

    fn remove_vorbis_tag(&mut self) {
        let _ = self.remove_vorbis_comments();
    }
}

impl VorbisTagContainer for OpusFile {
    fn vorbis_tag_mut(&mut self) -> Option<&mut VorbisComments> {
        Some(self.vorbis_comments_mut())
    }

    fn set_vorbis_tag(&mut self, tag: VorbisComments) {
        let _ = self.set_vorbis_comments(tag);
    }

    fn remove_vorbis_tag(&mut self) {
        let _ = self.remove_vorbis_comments();
    }
}

impl VorbisTagContainer for SpeexFile {
    fn vorbis_tag_mut(&mut self) -> Option<&mut VorbisComments> {
        Some(self.vorbis_comments_mut())
    }

    fn set_vorbis_tag(&mut self, tag: VorbisComments) {
        let _ = self.set_vorbis_comments(tag);
    }

    fn remove_vorbis_tag(&mut self) {
        let _ = self.remove_vorbis_comments();
    }
}

impl VorbisTagContainer for VorbisFile {
    fn vorbis_tag_mut(&mut self) -> Option<&mut VorbisComments> {
        Some(self.vorbis_comments_mut())
    }

    fn set_vorbis_tag(&mut self, tag: VorbisComments) {
        let _ = self.set_vorbis_comments(tag);
    }

    fn remove_vorbis_tag(&mut self) {
        let _ = self.remove_vorbis_comments();
    }
}

trait IlstTagContainer {
    fn ilst_tag_mut(&mut self) -> Option<&mut Ilst>;
    fn set_ilst_tag(&mut self, tag: Ilst);
    fn remove_ilst_tag(&mut self);
}

impl IlstTagContainer for Mp4File {
    fn ilst_tag_mut(&mut self) -> Option<&mut Ilst> {
        self.ilst_mut()
    }

    fn set_ilst_tag(&mut self, tag: Ilst) {
        let _ = self.set_ilst(tag);
    }

    fn remove_ilst_tag(&mut self) {
        let _ = self.remove_ilst();
    }
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

fn read_mp4_ilst(path: &Path) -> Option<Ilst> {
    let mut file = StdFile::open(path).ok()?;
    let mp4 = Mp4File::read_from(&mut file, supplemental_parse_options()).ok()?;
    mp4.ilst().cloned()
}

fn merge_mp4_ilst_tags(raw_tags: &mut BTreeMap<String, Vec<String>>, ilst: &Ilst) {
    for atom in ilst {
        if let AtomIdent::Freeform { mean, name } = atom.ident() {
            let key = normalize_tag_name(&format!("----:{mean}:{name}"));
            let mut extracted = Vec::new();
            for data in atom.data() {
                match data {
                    AtomData::UTF8(text) | AtomData::UTF16(text) => {
                        extracted.extend(split_multi_value(text));
                    }
                    _ => {}
                }
            }
            if !extracted.is_empty() {
                raw_tags.entry(key).or_default().extend(extracted);
            }
        }
    }
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
    let trimmed = tag.trim();
    if let Some(remainder) = trimmed.strip_prefix("----:") {
        if let Some((mean, name)) = remainder.split_once(':') {
            let mean_trimmed = mean.trim();
            let canonical_mean = if mean_trimmed.eq_ignore_ascii_case("com.apple.iTunes") {
                "com.apple.iTunes"
            } else {
                mean_trimmed
            };
            return format!("----:{canonical_mean}:{}", name.trim().to_ascii_uppercase());
        }
    }
    trimmed.to_ascii_uppercase()
}

fn normalize_root_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    #[cfg(target_os = "windows")]
    let normalized = trimmed.replace('/', "\\");
    #[cfg(not(target_os = "windows"))]
    let normalized = trimmed.to_string();

    if normalized.len() <= 3 {
        return normalized;
    }

    normalized.trim_end_matches(['\\', '/']).to_string()
}

fn root_paths_equal(left: &str, right: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        normalize_root_path(left).to_lowercase() == normalize_root_path(right).to_lowercase()
    }

    #[cfg(not(target_os = "windows"))]
    {
        normalize_root_path(left) == normalize_root_path(right)
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

fn build_tag_inventory(tracks: &[ScannedTrack]) -> Vec<TagInventoryEntry> {
    let mut inventory: HashMap<String, InventoryAccumulator> = HashMap::new();

    for track in tracks {
        merge_inventory(&mut inventory, &track.raw_tags);
    }

    let mut entries = inventory
        .into_iter()
        .map(|(tag, accumulator)| TagInventoryEntry {
            tag,
            occurrences: accumulator.occurrences,
            example_values: accumulator.example_values,
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        right
            .occurrences
            .cmp(&left.occurrences)
            .then_with(|| left.tag.cmp(&right.tag))
    });

    entries
}

fn map_fields(
    raw_tags: &BTreeMap<String, Vec<String>>,
    mappings: &[LibraryFieldMapping],
    track_format: &str,
    catalog_rules: &[CompiledCatalogRule],
) -> BTreeMap<String, Vec<String>> {
    let mut mapped = BTreeMap::new();
    let normalized_track_format = canonical_field_mapping_format(track_format);

    let default_mappings = mappings
        .iter()
        .filter(|mapping| canonical_field_mapping_format(&mapping.format) == "DEFAULT");
    let format_mappings = mappings.iter().filter(|mapping| {
        canonical_field_mapping_format(&mapping.format) == normalized_track_format
            && normalized_track_format != "DEFAULT"
    });

    for mapping in default_mappings.chain(format_mappings) {
        let mut resolved_values = Vec::new();

        for source_tag in &mapping.tag_priorities {
            let normalized = normalize_tag_name(source_tag);
            if let Some(values) = lookup_raw_tag_values(raw_tags, &normalized) {
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

fn lookup_raw_tag_values<'a>(
    raw_tags: &'a BTreeMap<String, Vec<String>>,
    normalized_key: &str,
) -> Option<&'a Vec<String>> {
    raw_tags.get(normalized_key).or_else(|| {
        raw_tags
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(normalized_key))
            .map(|(_, v)| v)
    })
}

fn extract_catalog_numbers(
    raw_tags: &BTreeMap<String, Vec<String>>,
    catalog_rules: &[CompiledCatalogRule],
) -> Vec<String> {
    let mut values = Vec::new();

    for tier in CATALOG_SOURCE_TAG_TIERS {
        let mut tier_source_values: Vec<&str> = Vec::new();
        for source in *tier {
            if let Some(source_values) = lookup_raw_tag_values(raw_tags, source) {
                for value in source_values {
                    if !value.trim().is_empty() {
                        tier_source_values.push(value.as_str());
                    }
                }
            }
        }

        if tier_source_values.is_empty() {
            continue;
        }

        let mut source_matches = Vec::new();
        for rule in catalog_rules {
            if !catalog_rule_matches_composer(raw_tags, rule) {
                continue;
            }

            for source_value in &tier_source_values {
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
        let matches = collect_catalog_matches(segment, regex);

        if !matches.is_empty() {
            return matches;
        }
    }

    collect_catalog_matches(source_value, regex)
}

fn collect_catalog_matches(value: &str, regex: &Regex) -> Vec<String> {
    regex
        .find_iter(value)
        .filter(|matched| !match_is_followed_by_catalog_range(value, matched.end()))
        .map(|matched| matched.as_str().trim().to_string())
        .collect()
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
                .map(|composer| deunicode::deunicode(composer.trim()).to_ascii_lowercase())
                .filter(|composer| !composer.is_empty())
                .collect::<Vec<_>>();
            let alnum_len = rule.label.chars().filter(|c| c.is_alphanumeric()).count();

            Some(CompiledCatalogRule {
                regex,
                composer_hints,
                is_single_letter_label: alnum_len <= 1,
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
        r"(?i)\b{prefix}\s*(?:(?:XXXII|XXXI|XXX|XXIX|XXVIII|XXVII|XXVI|XXV|XXIV|XXIII|XXII|XXI|XX|XIX|XVIII|XVII|XVI|XV|XIV|XIII|XII|XI|X|IX|VIII|VII|VI|V|IV|III|II|I)[ab]?\s*:?\s*)?\d+[A-Za-z]?(?:\s*[:.]\s*[A-Za-z0-9]+)?(?:\s*No\.?\s*\d+)?\b"
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
        .filter_map(|tag| lookup_raw_tag_values(raw_tags, tag))
        .flat_map(|values| values.iter())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    if composer_values.is_empty() {
        // Explicit multi-letter labels (e.g. HWV, BWV, WAB) can still match when composer is untagged,
        // while ambiguous single-letter labels (K, D, S.) require an explicit composer match.
        return !rule.is_single_letter_label;
    }

    composer_values.iter().any(|value| {
        let normalized = deunicode::deunicode(value).to_ascii_lowercase();
        rule.composer_hints
            .iter()
            .any(|hint| normalized.contains(hint))
    })
}

fn find_album_art(track_path: &Path, cache_root: &Path) -> Option<PathBuf> {
    match track_path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("flac") => {
            if let Some(embedded_art) = extract_embedded_flac_art(track_path, cache_root) {
                return Some(embedded_art);
            }
        }
        Some("m4a") | Some("mp4") => {
            if let Some(embedded_art) = extract_embedded_mp4_art(track_path, cache_root) {
                return Some(embedded_art);
            }
        }
        _ => {}
    }

    find_sidecar_album_art(track_path)
}

fn extract_embedded_flac_art(track_path: &Path, cache_root: &Path) -> Option<PathBuf> {
    let mut file = StdFile::open(track_path).ok()?;
    let flac = FlacFile::read_from(&mut file, cover_art_parse_options()).ok()?;
    let picture = choose_embedded_cover(flac.pictures())?;
    cache_embedded_picture(track_path, cache_root, picture)
}

fn extract_embedded_mp4_art(track_path: &Path, cache_root: &Path) -> Option<PathBuf> {
    let mut file = StdFile::open(track_path).ok()?;
    let mp4 = Mp4File::read_from(&mut file, cover_art_parse_options()).ok()?;
    let ilst = mp4.ilst()?;
    cache_first_mp4_picture(track_path, cache_root, ilst)
}

fn cache_first_mp4_picture(track_path: &Path, cache_root: &Path, ilst: &Ilst) -> Option<PathBuf> {
    let mut pictures = ilst.pictures()?;
    let picture = pictures.find(|picture| !picture.data().is_empty())?;
    cache_embedded_picture(track_path, cache_root, picture)
}

fn cache_embedded_picture(track_path: &Path, cache_root: &Path, picture: &Picture) -> Option<PathBuf> {
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
        cache_first_mp4_picture, collect_raw_tags_from_tag, compile_catalog_rules,
        discover_audio_files, extract_catalog_numbers, find_album_art, map_fields,
        merge_mp4_ilst_tags, normalize_raw_tag_values, normalize_tag_name,
        parse_mp4_freeform_ident, scan_library, scan_single_track, write_values_to_id3v2_tag,
        write_values_to_ilst_tag, write_values_to_vorbis_tag, LibraryService,
    };
    use std::{
        borrow::Cow,
        collections::BTreeMap,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    use aria_domain::{
        default_catalog_rules, default_field_mappings, LibraryRoot, LibrarySnapshot, ScannedTrack,
    };
    use lofty::{
        config::WriteOptions,
        file::AudioFile,
        flac::FlacFile,
        id3::v2::{FrameId, Id3v2Tag},
        iff::wav::RiffInfoList,
        mp4::{Atom, AtomData, AtomIdent, DataType, Ilst},
        ogg::{OggPictureStorage, VorbisComments},
        picture::{MimeType, Picture, PictureType},
        tag::{Tag, TagExt},
    };
    use tokio::sync::broadcast;

    fn write_minimal_flac(path: &Path, comments: &[(&str, &str)], picture: Option<Picture>) {
        // Minimal valid 42-byte FLAC file: "fLaC" + last STREAMINFO block (44.1 kHz, 2ch, 16-bit, 44100 samples)
        let mut flac_bytes = Vec::with_capacity(42);
        flac_bytes.extend_from_slice(b"fLaC");
        flac_bytes.extend_from_slice(&[0x80, 0x00, 0x00, 0x22]); // last block = true, type = STREAMINFO (0), len = 34
        flac_bytes.extend_from_slice(&4096u16.to_be_bytes()); // min block size
        flac_bytes.extend_from_slice(&4096u16.to_be_bytes()); // max block size
        flac_bytes.extend_from_slice(&[0u8; 6]); // min/max frame size
        // 20-bit sample rate (44100 = 0x0AC44), 3-bit channels-1 (1), 5-bit bps-1 (15), 36-bit total samples (44100)
        let packed: u64 = (44100u64 << 44) | (1u64 << 41) | (15u64 << 36) | 44100u64;
        flac_bytes.extend_from_slice(&packed.to_be_bytes());
        flac_bytes.extend_from_slice(&[0u8; 16]); // MD5
        std::fs::write(path, &flac_bytes).expect("minimal flac should be written");

        let mut vorbis = VorbisComments::new();
        for (key, value) in comments {
            vorbis.push((*key).to_string(), (*value).to_string());
        }
        vorbis
            .save_to_path(path, WriteOptions::default())
            .expect("vorbis comments should save to minimal flac");

        if let Some(pic) = picture {
            let mut file = std::fs::File::open(path).expect("flac should open for picture update");
            let mut flac = FlacFile::read_from(&mut file, lofty::config::ParseOptions::new())
                .expect("minimal flac should parse");
            drop(file);
            flac.insert_picture(pic, None)
                .expect("picture should insert into flac");
            lofty::file::AudioFile::save_to_path(&flac, path, WriteOptions::default())
                .expect("flac with picture should save");
        }
    }

    fn write_minimal_mp3(path: &Path, frames: &[(&str, &str)]) {
        // Minimal valid MPEG-1 Layer III stream (2 consecutive 128 kbps, 44.1 kHz, stereo frames = 834 bytes)
        let mut mp3_bytes = vec![0u8; 834];
        for offset in [0usize, 417usize] {
            mp3_bytes[offset] = 0xFF;
            mp3_bytes[offset + 1] = 0xFB;
            mp3_bytes[offset + 2] = 0x90;
            mp3_bytes[offset + 3] = 0x00;
        }
        std::fs::write(path, &mp3_bytes).expect("minimal mp3 should be written");

        let mut id3 = Id3v2Tag::new();
        for (key, value) in frames {
            write_values_to_id3v2_tag(&mut id3, key, &[(*value).to_string()]);
        }
        id3.save_to_path(path, WriteOptions::default())
            .expect("id3 tag should save to minimal mp3");
    }

    fn write_minimal_wav(path: &Path, entries: &[(&str, &str)]) {
        // Minimal valid PCM WAV file (44.1 kHz, 16-bit, stereo, 4 frames)
        let data_len: u32 = 16;
        let riff_len: u32 = 36 + data_len;
        let mut wav_bytes = Vec::with_capacity(44 + data_len as usize);
        wav_bytes.extend_from_slice(b"RIFF");
        wav_bytes.extend_from_slice(&riff_len.to_le_bytes());
        wav_bytes.extend_from_slice(b"WAVE");
        wav_bytes.extend_from_slice(b"fmt ");
        wav_bytes.extend_from_slice(&16u32.to_le_bytes());
        wav_bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav_bytes.extend_from_slice(&2u16.to_le_bytes()); // stereo
        wav_bytes.extend_from_slice(&44100u32.to_le_bytes()); // sample rate
        wav_bytes.extend_from_slice(&(44100u32 * 4).to_le_bytes()); // byte rate
        wav_bytes.extend_from_slice(&4u16.to_le_bytes()); // block align
        wav_bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav_bytes.extend_from_slice(b"data");
        wav_bytes.extend_from_slice(&data_len.to_le_bytes());
        wav_bytes.extend_from_slice(&vec![0u8; data_len as usize]);
        std::fs::write(path, &wav_bytes).expect("minimal wav should be written");

        let mut info = RiffInfoList::new();
        for (key, value) in entries {
            info.insert((*key).to_string(), (*value).to_string());
        }
        info.save_to_path(path, WriteOptions::default())
            .expect("riff info should save to minimal wav");
    }

    #[test]
    fn can_scan_library_across_flac_mp3_and_wav_fixtures() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("aria-fixture-scan-{unique}"));
        let library_root = temp_root.join("music");
        let cache_root = temp_root.join("cache");
        std::fs::create_dir_all(&library_root).expect("fixture dir should be created");

        write_minimal_flac(
            &library_root.join("01-bach.flac"),
            &[
                ("ALBUM", "Goldberg Variations"),
                ("TITLE", "Aria, BWV 988"),
                ("COMPOSER", "Johann Sebastian Bach"),
                ("PERFORMER", "Glenn Gould"),
            ],
            None,
        );
        write_minimal_mp3(
            &library_root.join("02-handel.mp3"),
            &[
                ("TALB", "Water Music"),
                ("TIT2", "Suite No. 1 in F Major, HWV 348"),
                ("TCOM", "Georg Friedrich Händel"),
                ("TPE1", "The English Concert"),
            ],
        );
        write_minimal_wav(
            &library_root.join("03-mozart.wav"),
            &[
                ("IPRD", "Symphonies Nos. 40 & 41"),
                ("INAM", "Symphony No. 41 in C Major, K. 551"),
                ("IART", "Berlin Philharmonic"),
            ],
        );

        let (events, _) = broadcast::channel(64);
        let artifacts = scan_library(
            &[LibraryRoot {
                path: library_root.to_string_lossy().into_owned(),
                label: "Fixture Library".into(),
            }],
            &default_field_mappings(),
            &default_catalog_rules(),
            &cache_root,
            &events,
        )
        .expect("fixture library should scan");

        assert_eq!(artifacts.tracks.len(), 3, "expected all 3 format fixtures to scan");
        assert!(
            artifacts.tag_inventory.iter().any(|entry| entry.tag == "ALBUM"),
            "expected ALBUM tag in inventory"
        );

        let flac = artifacts
            .tracks
            .iter()
            .find(|t| t.file_name == "01-bach.flac")
            .expect("flac fixture should be scanned");
        assert_eq!(flac.mapped_fields.get("album"), Some(&vec!["Goldberg Variations".into()]));
        assert_eq!(flac.mapped_fields.get("catalog"), Some(&vec!["BWV 988".into()]));

        let mp3 = artifacts
            .tracks
            .iter()
            .find(|t| t.file_name == "02-handel.mp3")
            .expect("mp3 fixture should be scanned");
        assert_eq!(mp3.mapped_fields.get("album"), Some(&vec!["Water Music".into()]));
        assert_eq!(mp3.mapped_fields.get("composer"), Some(&vec!["Georg Friedrich Händel".into()]));
        assert_eq!(mp3.mapped_fields.get("catalog"), Some(&vec!["HWV 348".into()]));

        let wav = artifacts
            .tracks
            .iter()
            .find(|t| t.file_name == "03-mozart.wav")
            .expect("wav fixture should be scanned");
        assert_eq!(wav.mapped_fields.get("album"), Some(&vec!["Symphonies Nos. 40 & 41".into()]));
        assert_eq!(
            wav.mapped_fields.get("title"),
            Some(&vec!["Symphony No. 41 in C Major, K. 551".into()])
        );

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn preserves_custom_flac_vorbis_fields_during_scan() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("aria-flac-custom-tags-{unique}"));
        let cache_root = temp_root.join("cache");
        std::fs::create_dir_all(&temp_root).expect("temp dir should be created");

        let flac_path = temp_root.join("05-berg-concerto.flac");
        write_minimal_flac(
            &flac_path,
            &[
                ("ALBUM", "Brahms & Berg: Violin Concertos"),
                ("TITLE", "Violin Concerto: II. Allegro - Adagio"),
                ("COMPOSER", "Alban Berg"),
                ("ENSEMBLE", "Vienna Philharmonic"),
                ("CONDUCTOR", "Daniel Harding"),
            ],
            None,
        );

        let track = scan_single_track(
            &flac_path,
            &default_field_mappings(),
            &compile_catalog_rules(&default_catalog_rules()),
            &cache_root,
        )
        .expect("flac fixture should scan");

        assert!(
            track.raw_tags.contains_key("ENSEMBLE"),
            "expected ENSEMBLE raw tag to be preserved"
        );
        assert_eq!(
            track.mapped_fields.get("ensemble"),
            Some(&vec!["Vienna Philharmonic".to_string()]),
            "expected ensemble field mapping to resolve"
        );

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn discover_audio_files_skips_hidden_and_system_dirs() {
        let temp_dir = std::env::temp_dir().join(format!(
            "discover-audio-files-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();

        // Create standard folder
        let normal_dir = temp_dir.join("music");
        std::fs::create_dir_all(&normal_dir).unwrap();
        std::fs::write(normal_dir.join("song.mp3"), b"").unwrap();

        // Create skipped folders (by name)
        let recycle_dir = temp_dir.join("$RECYCLE.BIN");
        std::fs::create_dir_all(&recycle_dir).unwrap();
        std::fs::write(recycle_dir.join("trash.mp3"), b"").unwrap();

        let sys_vol_dir = temp_dir.join("System Volume Information");
        std::fs::create_dir_all(&sys_vol_dir).unwrap();
        std::fs::write(sys_vol_dir.join("sys.mp3"), b"").unwrap();

        // Create dot folder
        let dot_dir = temp_dir.join(".hidden");
        std::fs::create_dir_all(&dot_dir).unwrap();
        std::fs::write(dot_dir.join("hidden_song.mp3"), b"").unwrap();

        let roots = vec![LibraryRoot {
            path: temp_dir.to_string_lossy().into_owned(),
            label: "Temp".into(),
        }];

        let discovered = super::discover_audio_files(&roots);

        // Verify only the normal song.mp3 is discovered
        assert_eq!(discovered.len(), 1);
        assert_eq!(
            discovered[0].file_name().unwrap().to_string_lossy(),
            "song.mp3"
        );

        // Cleanup
        std::fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn maps_mp4_standard_and_freeform_atoms_end_to_end() {
        let mut ilst = Ilst::new();
        ilst.insert(Atom::new(
            AtomIdent::Fourcc(*b"\xa9alb"),
            AtomData::UTF8("Bach Cantatas, Vol. 30".into()),
        ));
        ilst.insert(Atom::new(
            AtomIdent::Fourcc(*b"\xa9nam"),
            AtomData::UTF8("Cantata BWV 51: Jauchzet Gott in allen Landen".into()),
        ));
        ilst.insert(Atom::new(
            AtomIdent::Fourcc(*b"\xa9wrt"),
            AtomData::UTF8("Johann Sebastian Bach".into()),
        ));
        ilst.insert(Atom::new(
            AtomIdent::Fourcc(*b"\xa9ART"),
            AtomData::UTF8("Carolyn Sampson".into()),
        ));
        ilst.insert(Atom::new(
            AtomIdent::Fourcc(*b"\xa9day"),
            AtomData::UTF8("2005".into()),
        ));
        ilst.insert(Atom::new(
            AtomIdent::Fourcc(*b"trkn"),
            AtomData::Unknown {
                code: DataType::Reserved,
                data: vec![0, 0, 0, 1, 0, 12, 0, 0],
            },
        ));
        ilst.insert(Atom::new(
            AtomIdent::Fourcc(*b"disk"),
            AtomData::Unknown {
                code: DataType::Reserved,
                data: vec![0, 0, 0, 1, 0, 2],
            },
        ));
        ilst.insert(Atom::new(
            AtomIdent::Freeform {
                mean: Cow::Borrowed("com.apple.iTunes"),
                name: Cow::Borrowed("ENSEMBLE"),
            },
            AtomData::UTF8("Bach Collegium Japan".into()),
        ));

        let mut raw_tags = BTreeMap::new();
        let generic_tag: Tag = ilst.clone().into();
        collect_raw_tags_from_tag(&generic_tag, &mut raw_tags);
        merge_mp4_ilst_tags(&mut raw_tags, &ilst);
        normalize_raw_tag_values(&mut raw_tags);

        let mapped = map_fields(
            &raw_tags,
            &default_field_mappings(),
            "MP4",
            &compile_catalog_rules(&default_catalog_rules()),
        );

        assert_eq!(mapped.get("album"), Some(&vec!["Bach Cantatas, Vol. 30".into()]));
        assert_eq!(
            mapped.get("title"),
            Some(&vec!["Cantata BWV 51: Jauchzet Gott in allen Landen".into()])
        );
        assert_eq!(
            mapped.get("composer"),
            Some(&vec!["Johann Sebastian Bach".into()])
        );
        assert_eq!(mapped.get("soloist"), Some(&vec!["Carolyn Sampson".into()]));
        assert_eq!(
            mapped.get("ensemble"),
            Some(&vec!["Bach Collegium Japan".into()]),
            "raw_tags were: {raw_tags:?}"
        );
        assert_eq!(mapped.get("year"), Some(&vec!["2005".into()]));
        assert_eq!(mapped.get("catalog"), Some(&vec!["BWV 51".into()]));
    }

    #[test]
    fn embedded_mp4_picture_is_cached_when_present() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("aria-mp4-art-test-{unique}"));
        let cache_root = temp_root.join("cache");
        let track_path = temp_root.join("track.m4a");
        std::fs::create_dir_all(&temp_root).expect("temp directory should be created");
        std::fs::write(&track_path, b"placeholder").expect("placeholder track should be written");

        let mut ilst = Ilst::new();
        ilst.insert_picture(
            Picture::unchecked(vec![0xFF, 0xD8, 0xFF, 0xD9])
                .pic_type(PictureType::Other)
                .mime_type(MimeType::Jpeg)
                .build(),
        );

        let album_art = cache_first_mp4_picture(&track_path, &cache_root, &ilst)
            .expect("expected embedded mp4 art to be cached");

        assert!(album_art.starts_with(&cache_root));
        assert_eq!(album_art.extension().and_then(|extension| extension.to_str()), Some("jpg"));
        assert!(
            std::fs::metadata(&album_art)
                .map(|metadata| metadata.len() > 0)
                .unwrap_or(false),
            "expected cached embedded art to be non-empty"
        );

        std::fs::remove_dir_all(&temp_root).ok();
    }

    #[test]
    fn embedded_flac_art_wins_over_empty_sidecar() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("aria-flac-art-test-{unique}"));
        let cache_root = temp_root.join("cache");
        std::fs::create_dir_all(&temp_root).expect("temp directory should be created");

        let flac_path = temp_root.join("01-allegro.flac");
        let embedded_pic = Picture::unchecked(vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0xFF, 0xD9])
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Jpeg)
            .build();
        write_minimal_flac(
            &flac_path,
            &[("ALBUM", "Bach Keyboard Concertos")],
            Some(embedded_pic),
        );
        // Empty sidecar file should NOT override embedded FLAC art
        std::fs::write(temp_root.join("cover.jpg"), []).expect("empty sidecar should be written");

        let album_art = find_album_art(&flac_path, &cache_root)
            .expect("flac fixture should resolve embedded album art");

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

        let _ = std::fs::remove_dir_all(&temp_root);
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
    fn mp4_sidecar_is_used_when_embedded_art_is_missing() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("aria-mp4-sidecar-test-{unique}"));
        let cache_root = temp_root.join("cache");
        std::fs::create_dir_all(&temp_root).expect("temp directory should be created");

        let track_path = temp_root.join("track.m4a");
        std::fs::write(&track_path, b"not a real mp4 but enough to force sidecar fallback")
            .expect("track placeholder should be written");
        std::fs::write(temp_root.join("cover.jpg"), [0xFF, 0xD8, 0xFF, 0xD9])
            .expect("sidecar art should be written");

        let album_art = find_album_art(&track_path, &cache_root)
            .expect("expected sidecar art fallback for mp4 track without embedded art");

        assert_eq!(album_art, temp_root.join("cover.jpg"));

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

    #[test]
    fn haydn_hob_catalog_extracts_sectioned_catalog_number() {
        let rules = compile_catalog_rules(&default_catalog_rules());
        let raw_tags = BTreeMap::from([
            ("COMPOSER".into(), vec!["Joseph Haydn".into()]),
            (
                "TITLE".into(),
                vec!["String Quartet in C major, Hob. IIIb:2".into()],
            ),
        ]);

        let values = extract_catalog_numbers(&raw_tags, &rules);

        assert_eq!(values, vec!["Hob. IIIb:2"]);
    }

    #[test]
    fn haydn_hob_catalog_allows_missing_colon_before_number() {
        let rules = compile_catalog_rules(&default_catalog_rules());
        let raw_tags = BTreeMap::from([
            ("COMPOSER".into(), vec!["Franz Joseph Haydn".into()]),
            (
                "TITLE".into(),
                vec!["Piano Sonata in E-flat major, Hob. XVI 52".into()],
            ),
        ]);

        let values = extract_catalog_numbers(&raw_tags, &rules);

        assert_eq!(values, vec!["Hob. XVI 52"]);
    }

    #[test]
    fn id3v2_export_updates_known_frame_values() {
        let mut tag = Id3v2Tag::new();
        write_values_to_id3v2_tag(
            &mut tag,
            "TCOM",
            &["Claude Debussy".into(), "Maurice Ravel".into()],
        );

        let frame_id = FrameId::new("TCOM").expect("TCOM should be a valid frame id");
        let composers = tag
            .get_text(&frame_id)
            .expect("composer frame should be written")
            .split('\0')
            .collect::<Vec<_>>();
        assert_eq!(composers, vec!["Claude Debussy", "Maurice Ravel"]);
    }

    #[test]
    fn id3v2_export_uses_user_text_for_custom_tags() {
        let mut tag = Id3v2Tag::new();
        write_values_to_id3v2_tag(
            &mut tag,
            "CATALOG",
            &["BWV 1007".into(), "BWV 1008".into()],
        );

        assert_eq!(tag.get_user_text("CATALOG"), Some("BWV 1007; BWV 1008"));
    }

    #[test]
    fn vorbis_export_replaces_target_tag_with_all_values() {
        let mut tag = VorbisComments::new();
        tag.push("CATALOGNUMBER".into(), "OLD 1".into());
        tag.push("CATALOGNUMBER".into(), "OLD 2".into());

        write_values_to_vorbis_tag(
            &mut tag,
            "CATALOGNUMBER",
            &["HOB. XVI 52".into(), "OP. 57".into()],
        );

        let values = tag.get_all("CATALOGNUMBER").collect::<Vec<_>>();
        assert_eq!(values, vec!["HOB. XVI 52", "OP. 57"]);
    }

    #[test]
    fn mp4_freeform_ident_parser_preserves_explicit_mean_and_name() {
        let ident = parse_mp4_freeform_ident("----:com.apple.iTunes:CATALOGNUMBER");

        assert!(matches!(
            ident,
            AtomIdent::Freeform { mean, name }
                if mean == "com.apple.iTunes" && name == "CATALOGNUMBER"
        ));

        let canonicalized = parse_mp4_freeform_ident("----:COM.APPLE.ITUNES:ENSEMBLE");
        assert!(matches!(
            canonicalized,
            AtomIdent::Freeform { mean, name }
                if mean == "com.apple.iTunes" && name == "ENSEMBLE"
        ));
    }

    #[test]
    fn normalize_tag_name_preserves_mp4_itunes_mean_case() {
        assert_eq!(
            normalize_tag_name("----:com.apple.iTunes:ENSEMBLE"),
            "----:com.apple.iTunes:ENSEMBLE"
        );
        assert_eq!(
            normalize_tag_name("----:COM.APPLE.ITUNES:conductor"),
            "----:com.apple.iTunes:CONDUCTOR"
        );
        assert_eq!(normalize_tag_name("©alb"), "©ALB");
    }

    #[test]
    fn ilst_export_replaces_legacy_uppercased_freeform_atom_without_duplicates() {
        let mut tag = Ilst::new();
        tag.insert(Atom::new(
            AtomIdent::Freeform {
                mean: Cow::Owned("COM.APPLE.ITUNES".into()),
                name: Cow::Owned("ENSEMBLE".into()),
            },
            AtomData::UTF8("Old Orchestra".into()),
        ));

        write_values_to_ilst_tag(
            &mut tag,
            "----:com.apple.iTunes:ENSEMBLE",
            &["Berliner Philharmoniker".into()],
        );

        let matching: Vec<_> = (&tag)
            .into_iter()
            .filter(|atom| {
                matches!(
                    atom.ident(),
                    AtomIdent::Freeform { mean, name }
                        if mean.eq_ignore_ascii_case("com.apple.iTunes")
                            && name.eq_ignore_ascii_case("ENSEMBLE")
                )
            })
            .collect();
        assert_eq!(matching.len(), 1);
        assert!(matches!(
            matching[0].ident(),
            AtomIdent::Freeform { mean, name }
                if mean == "com.apple.iTunes" && name == "ENSEMBLE"
        ));
    }

    #[test]
    fn id3v2_and_mp4_mappings_and_catalog_extraction_work_out_of_the_box() {
        let mappings = aria_domain::default_field_mappings();
        let rules = compile_catalog_rules(&default_catalog_rules());

        // MP3 with native ID3v2 frame IDs + non-ASCII composer (Händel)
        let mp3_raw = BTreeMap::from([
            ("TALB".into(), vec!["Messiah".into()]),
            ("TIT2".into(), vec!["Messiah, HWV 56: Hallelujah".into()]),
            ("TCOM".into(), vec!["Georg Friedrich Händel".into()]),
            ("TPE3".into(), vec!["John Eliot Gardiner".into()]),
            ("TPE1".into(), vec!["Monteverdi Choir".into()]),
            ("TDRC".into(), vec!["1983".into()]),
            ("TPOS".into(), vec!["1/2".into()]),
            ("TRCK".into(), vec!["12".into()]),
        ]);
        let mp3_mapped = map_fields(&mp3_raw, &mappings, "MP3", &rules);
        assert_eq!(mp3_mapped.get("album"), Some(&vec!["Messiah".to_string()]));
        assert_eq!(mp3_mapped.get("composer"), Some(&vec!["Georg Friedrich Händel".to_string()]));
        assert_eq!(mp3_mapped.get("conductor"), Some(&vec!["John Eliot Gardiner".to_string()]));
        assert_eq!(mp3_mapped.get("catalog"), Some(&vec!["HWV 56".to_string()]));
        assert_eq!(mp3_mapped.get("year"), Some(&vec!["1983".to_string()]));
        assert_eq!(mp3_mapped.get("track_number"), Some(&vec!["12".to_string()]));

        // MP4 with native atoms (©NAM, ©WRT)
        let mp4_raw = BTreeMap::from([
            ("©ALB".into(), vec!["Mozart Piano Concertos".into()]),
            ("©NAM".into(), vec!["Piano Concerto No. 20 in D Minor, K. 466: I. Allegro".into()]),
            ("©WRT".into(), vec!["Wolfgang Amadeus Mozart".into()]),
        ]);
        let mp4_mapped = map_fields(&mp4_raw, &mappings, "MP4", &rules);
        assert_eq!(mp4_mapped.get("catalog"), Some(&vec!["K. 466".to_string()]));

        // Ambiguous single-letter catalog (K) must NOT match when composer is missing
        let untagged_raw = BTreeMap::from([("TITLE".into(), vec!["Track K 466".into()])]);
        let untagged_catalog = extract_catalog_numbers(&untagged_raw, &rules);
        assert!(untagged_catalog.is_empty());
    }

    #[tokio::test]
    async fn set_field_mappings_and_catalog_rules_preserve_remote_album_path_and_empty_raw_tags() {
        let mut remote_with_raw = ScannedTrack {
            id: "remote_1".into(),
            path: "remote://t1/Bach/01.flac".into(),
            ..Default::default()
        };
        remote_with_raw.audio.format = "FLAC".into();
        remote_with_raw.raw_tags.insert("ALBUM".into(), vec!["Goldberg".into()]);
        remote_with_raw
            .mapped_fields
            .insert("album_path".into(), vec!["Bach/Goldberg".into()]);

        let mut remote_without_raw = ScannedTrack {
            id: "remote_2".into(),
            path: "remote://t1/Mozart/01.flac".into(),
            ..Default::default()
        };
        remote_without_raw.audio.format = "FLAC".into();
        remote_without_raw
            .mapped_fields
            .insert("album".into(), vec!["Requiem".into()]);
        remote_without_raw
            .mapped_fields
            .insert("album_path".into(), vec!["Mozart/Requiem".into()]);

        let snapshot = LibrarySnapshot {
            tracks: vec![remote_with_raw, remote_without_raw],
            ..Default::default()
        };
        let service = LibraryService::with_snapshot(snapshot);

        let after_mappings = service
            .set_field_mappings(aria_domain::default_field_mappings())
            .await;
        assert_eq!(
            after_mappings.tracks[0].mapped_fields.get("album_path"),
            Some(&vec!["Bach/Goldberg".to_string()])
        );
        assert_eq!(
            after_mappings.tracks[1].mapped_fields.get("album"),
            Some(&vec!["Requiem".to_string()])
        );
        assert_eq!(
            after_mappings.tracks[1].mapped_fields.get("album_path"),
            Some(&vec!["Mozart/Requiem".to_string()])
        );

        let after_rules = service
            .set_catalog_rules(default_catalog_rules())
            .await
            .expect("set catalog rules");
        assert_eq!(
            after_rules.tracks[0].mapped_fields.get("album_path"),
            Some(&vec!["Bach/Goldberg".to_string()])
        );
        assert_eq!(
            after_rules.tracks[1].mapped_fields.get("album"),
            Some(&vec!["Requiem".to_string()])
        );
    }

    #[test]
    fn discover_audio_files_deduplicates_overlapping_roots() {
        let base = std::env::temp_dir().join(format!(
            "aria_overlap_roots_test_{}",
            std::process::id()
        ));
        let sub = base.join("Bach");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&sub).unwrap();
        let track = sub.join("01.flac");
        std::fs::write(&track, b"stub").unwrap();

        let roots = vec![
            LibraryRoot {
                path: base.to_string_lossy().to_string(),
                label: "base".into(),
            },
            LibraryRoot {
                path: sub.to_string_lossy().to_string(),
                label: "sub".into(),
            },
        ];
        let discovered = discover_audio_files(&roots);
        assert_eq!(discovered.len(), 1);

        let _ = std::fs::remove_dir_all(&base);
    }
}
