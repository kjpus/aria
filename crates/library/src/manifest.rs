use std::fs::File;
use std::io::{Read, Result as IoResult};
use std::path::Path;

use aria_domain::{
    AlbumManifest, ManifestTrack, MasterAlbumSummary, MasterLibraryIndex, MasterPlaylistIndex,
    Playlist, RemotePlaylistManifest, RemotePlaylistSummary, RemotePlaylistTrack, ScannedTrack,
    CURRENT_MANIFEST_VERSION,
};
use chrono::Utc;
use sha2::{Digest, Sha256};

/// Sanitizes a string so it is safe to use as a folder or file name across platforms
/// (Windows, Linux, macOS, Google Drive, SMB).
pub fn sanitize_path_segment(segment: &str) -> String {
    let sanitized: String = segment
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            '\0'..='\x1f' => '_',
            other => other,
        })
        .collect();

    let trimmed = sanitized.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Matches common disc folder patterns like "Disc 1", "Disc 02", "CD 1", "CD01", "Disk 1", "Volume 1", "Vol. 1", "Part 1".
pub fn is_disc_directory_name(name: &str) -> bool {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"(?i)^(?:cd|disc|disk|vol|volume|part)[.\s_-]*0*[0-9]+").unwrap()
    });
    re.is_match(name.trim())
}

/// Returns the effective album directory for a track path, collapsing disc subdirectories (e.g. "Disc 1") to their parent.
pub fn effective_album_directory(path: &str) -> String {
    let last_sep = path.rfind(|c| c == '/' || c == '\\');
    let parent = match last_sep {
        Some(idx) => &path[..idx],
        None => return String::new(),
    };

    let parent_sep = parent.rfind(|c| c == '/' || c == '\\');
    let folder_name = match parent_sep {
        Some(idx) => &parent[idx + 1..],
        None => parent,
    };

    if is_disc_directory_name(folder_name) {
        if let Some(idx) = parent_sep {
            return parent[..idx].to_string();
        }
    }

    parent.to_string()
}

/// Derives a consistent album ID for a track based on its album title, effective album directory, and performance metadata.
pub fn album_id_for_track(track: &ScannedTrack) -> String {
    let title = track
        .mapped_fields
        .get("album")
        .and_then(|a| a.first())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| track.file_name.trim().to_lowercase());

    let dir = effective_album_directory(&track.path)
        .trim()
        .to_lowercase()
        .replace('\\', "/");

    let conductor = track
        .mapped_fields
        .get("conductor")
        .and_then(|v| v.first())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();

    let ensemble = track
        .mapped_fields
        .get("ensemble")
        .and_then(|v| v.first())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();

    let soloist = track
        .mapped_fields
        .get("soloist")
        .and_then(|v| v.first())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();

    let year = track
        .mapped_fields
        .get("year")
        .and_then(|v| v.first())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();

    let catalog = track
        .mapped_fields
        .get("catalog")
        .and_then(|v| v.first())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();

    let performer = if !conductor.is_empty() && !ensemble.is_empty() {
        format!("{conductor} / {ensemble}")
    } else if !conductor.is_empty() {
        conductor
    } else if !ensemble.is_empty() {
        ensemble
    } else {
        soloist
    };

    let mut parts = Vec::new();
    if !title.is_empty() {
        parts.push(title);
    }
    if !dir.is_empty() {
        parts.push(dir);
    }
    if !performer.is_empty() {
        parts.push(performer);
    }
    if !year.is_empty() {
        parts.push(year);
    }
    if !catalog.is_empty() {
        parts.push(catalog);
    }

    if !parts.is_empty() {
        parts.join("::")
    } else {
        track.id.clone()
    }
}

/// Derives a deterministic canonical album ID based on album title, composers, and catalog numbers.
pub fn derive_canonical_album_id(
    title: &str,
    composers: &[String],
    catalog_numbers: &[String],
) -> String {
    derive_canonical_album_id_full(title, composers, &[], &[], &[], None, catalog_numbers)
}

/// Derives a deterministic canonical album ID taking into account performers and recording year.
pub fn derive_canonical_album_id_full(
    title: &str,
    composers: &[String],
    conductors: &[String],
    ensembles: &[String],
    soloists: &[String],
    year: Option<&str>,
    catalog_numbers: &[String],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.trim().to_lowercase().as_bytes());
    hasher.update(b"|");

    let mut sorted_composers = composers.to_vec();
    sorted_composers.sort();
    for c in &sorted_composers {
        hasher.update(c.trim().to_lowercase().as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"|");

    let mut sorted_conductors = conductors.to_vec();
    sorted_conductors.sort();
    for c in &sorted_conductors {
        hasher.update(c.trim().to_lowercase().as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"|");

    let mut sorted_ensembles = ensembles.to_vec();
    sorted_ensembles.sort();
    for e in &sorted_ensembles {
        hasher.update(e.trim().to_lowercase().as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"|");

    let mut sorted_soloists = soloists.to_vec();
    sorted_soloists.sort();
    for s in &sorted_soloists {
        hasher.update(s.trim().to_lowercase().as_bytes());
        hasher.update(b",");
    }
    hasher.update(b"|");

    if let Some(y) = year {
        hasher.update(y.trim().to_lowercase().as_bytes());
    }
    hasher.update(b"|");

    let mut sorted_catalogs = catalog_numbers.to_vec();
    sorted_catalogs.sort();
    for cat in &sorted_catalogs {
        hasher.update(cat.trim().to_lowercase().as_bytes());
        hasher.update(b",");
    }

    let hash_bytes = hasher.finalize();
    format!("alb_{}", hex_encode(&hash_bytes[..12]))
}

/// Derives a deterministic canonical track ID based on album ID, disk number, track number, title, and file name.
pub fn derive_canonical_track_id(
    album_id: &str,
    disk_number: Option<&str>,
    track_number: Option<&str>,
    title: &str,
    file_name: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(album_id.as_bytes());
    hasher.update(b"|");
    hasher.update(disk_number.unwrap_or("").trim().as_bytes());
    hasher.update(b"|");
    hasher.update(track_number.unwrap_or("").trim().as_bytes());
    hasher.update(b"|");
    hasher.update(title.trim().to_lowercase().as_bytes());
    hasher.update(b"|");
    hasher.update(file_name.trim().to_lowercase().as_bytes());

    let hash_bytes = hasher.finalize();
    format!("trk_{}", hex_encode(&hash_bytes[..12]))
}

/// Computes SHA-256 checksum of a file.
pub fn compute_file_sha256(path: &Path) -> IoResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash_bytes = hasher.finalize();
    Ok(hex_encode(&hash_bytes))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Extracts unique strings from a mapped field across multiple tracks.
fn extract_unique_mapped_fields(tracks: &[ScannedTrack], field_key: &str) -> Vec<String> {
    let mut set = Vec::new();
    for track in tracks {
        if let Some(values) = track.mapped_fields.get(field_key) {
            for val in values {
                let trimmed = val.trim();
                if !trimmed.is_empty() && !set.iter().any(|existing: &String| existing == trimmed) {
                    set.push(trimmed.to_string());
                }
            }
        }
    }
    set
}

/// Generates an `AlbumManifest` from a set of scanned tracks representing an album.
pub fn build_album_manifest(
    album_title: &str,
    tracks: &[ScannedTrack],
    artwork_file_name: Option<String>,
    compute_checksums: bool,
) -> IoResult<AlbumManifest> {
    let composers = extract_unique_mapped_fields(tracks, "composer");
    let conductors = extract_unique_mapped_fields(tracks, "conductor");
    let ensembles = extract_unique_mapped_fields(tracks, "ensemble");
    let soloists = extract_unique_mapped_fields(tracks, "soloist");
    let catalog_numbers = extract_unique_mapped_fields(tracks, "catalog");
    let years = extract_unique_mapped_fields(tracks, "year");
    let year = years.into_iter().next();

    let album_id = derive_canonical_album_id_full(
        album_title,
        &composers,
        &conductors,
        &ensembles,
        &soloists,
        year.as_deref(),
        &catalog_numbers,
    );

    let mut manifest_tracks = Vec::with_capacity(tracks.len());

    for track in tracks {
        let path = Path::new(&track.path);
        let file_size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        let checksum = if compute_checksums && path.is_file() {
            compute_file_sha256(path).ok()
        } else {
            None
        };

        let title = track
            .mapped_fields
            .get("title")
            .and_then(|t| t.first())
            .map(|s| s.as_str())
            .unwrap_or(&track.file_name);

        let disk_number = track
            .mapped_fields
            .get("disk_number")
            .and_then(|d| d.first())
            .map(|s| s.as_str());

        let track_number = track
            .mapped_fields
            .get("track_number")
            .and_then(|t| t.first())
            .map(|s| s.as_str());

        let track_id = derive_canonical_track_id(
            &album_id,
            disk_number,
            track_number,
            title,
            &track.file_name,
        );

        // Derive systematic relative file name: e.g. "01 - Allegro.flac"
        let relative_file_name = derive_systematic_file_name(disk_number, track_number, title, &track.file_name);

        manifest_tracks.push(ManifestTrack {
            track_id,
            relative_file_name,
            file_name: track.file_name.clone(),
            file_size_bytes,
            audio_checksum_sha256: checksum,
            audio: track.audio.clone(),
            raw_tags: track.raw_tags.clone(),
            mapped_fields: track.mapped_fields.clone(),
        });
    }

    Ok(AlbumManifest {
        manifest_version: CURRENT_MANIFEST_VERSION,
        album_id,
        title: album_title.to_string(),
        composers,
        conductors,
        ensembles,
        soloists,
        catalog_numbers,
        year,
        artwork_file_name,
        updated_at: Utc::now().to_rfc3339(),
        tracks: manifest_tracks,
    })
}

/// Builds a systematic audio file name: e.g., "1-01 - Title.flac" or "01 - Title.flac".
pub fn derive_systematic_file_name(
    disk_number: Option<&str>,
    track_number: Option<&str>,
    title: &str,
    original_file_name: &str,
) -> String {
    let ext = Path::new(original_file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("flac");

    let clean_title = sanitize_path_segment(title);

    match (disk_number, track_number) {
        (Some(d), Some(t)) if !d.trim().is_empty() && d.trim() != "1" => {
            let disk_padded = d.trim();
            let track_padded = format!("{:0>2}", t.trim());
            format!("{}-{} - {}.{}", disk_padded, track_padded, clean_title, ext)
        }
        (_, Some(t)) if !t.trim().is_empty() => {
            let track_padded = format!("{:0>2}", t.trim());
            format!("{} - {}.{}", track_padded, clean_title, ext)
        }
        _ => sanitize_path_segment(original_file_name),
    }
}

/// Derives the systematic folder path for an album: e.g., "Composer/Album" or "Composer/Album (Performers - Year)".
pub fn derive_systematic_folder_name(manifest: &AlbumManifest) -> String {
    let composer = manifest
        .composers
        .first()
        .map(|s| sanitize_path_segment(s))
        .unwrap_or_else(|| "Various Composers".to_string());

    let album = sanitize_path_segment(&manifest.title);

    let conductor = manifest.conductors.first().map(|s| s.trim()).filter(|s| !s.is_empty());
    let ensemble = manifest.ensembles.first().map(|s| s.trim()).filter(|s| !s.is_empty());
    let soloist = manifest.soloists.first().map(|s| s.trim()).filter(|s| !s.is_empty());
    let year = manifest.year.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty());

    let performer = if let (Some(c), Some(e)) = (conductor, ensemble) {
        Some(format!("{c}, {e}"))
    } else if let Some(c) = conductor {
        Some(c.to_string())
    } else if let Some(e) = ensemble {
        Some(e.to_string())
    } else {
        soloist.map(|s| s.to_string())
    };

    let disambiguator = match (performer, year) {
        (Some(p), Some(y)) => Some(format!("{p} - {y}")),
        (Some(p), None) => Some(p),
        (None, Some(y)) => Some(y.to_string()),
        (None, None) => None,
    };

    match disambiguator {
        Some(dis) => {
            if manifest.title.to_lowercase().contains(&dis.to_lowercase()) {
                format!("{}/{}", composer, album)
            } else {
                let clean_dis = sanitize_path_segment(&dis);
                format!("{}/{} ({})", composer, album, clean_dis)
            }
        }
        None => format!("{}/{}", composer, album),
    }
}

/// Creates a `MasterAlbumSummary` from an `AlbumManifest`.
pub fn build_master_album_summary(manifest: &AlbumManifest, relative_folder: &str) -> MasterAlbumSummary {
    let total_duration_ms = manifest.tracks.iter().map(|t| t.audio.duration_ms).sum();
    let total_size_bytes = manifest.tracks.iter().map(|t| t.file_size_bytes).sum();

    MasterAlbumSummary {
        album_id: manifest.album_id.clone(),
        title: manifest.title.clone(),
        composers: manifest.composers.clone(),
        conductors: manifest.conductors.clone(),
        ensembles: manifest.ensembles.clone(),
        soloists: manifest.soloists.clone(),
        catalog_numbers: manifest.catalog_numbers.clone(),
        year: manifest.year.clone(),
        relative_folder: relative_folder.to_string(),
        artwork_file_name: manifest.artwork_file_name.clone(),
        track_count: manifest.tracks.len(),
        total_duration_ms,
        total_size_bytes,
        updated_at: Some(manifest.updated_at.clone()),
    }
}

/// Builds a `MasterLibraryIndex` from a list of album summaries.
pub fn build_master_library_index(albums: Vec<MasterAlbumSummary>) -> MasterLibraryIndex {
    MasterLibraryIndex {
        manifest_version: CURRENT_MANIFEST_VERSION,
        updated_at: Utc::now().to_rfc3339(),
        albums,
    }
}

pub fn serialize_manifest(manifest: &AlbumManifest) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(manifest)
}

pub fn deserialize_manifest(json: &str) -> Result<AlbumManifest, serde_json::Error> {
    serde_json::from_str(json)
}

pub fn serialize_master_index(index: &MasterLibraryIndex) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(index)
}

pub fn deserialize_master_index(json: &str) -> Result<MasterLibraryIndex, serde_json::Error> {
    serde_json::from_str(json)
}

/// Creates a `RemotePlaylistManifest` from a `Playlist` and resolved `RemotePlaylistTrack`s.
pub fn build_remote_playlist_manifest(
    playlist: &Playlist,
    tracks: Vec<RemotePlaylistTrack>,
) -> RemotePlaylistManifest {
    RemotePlaylistManifest {
        manifest_version: CURRENT_MANIFEST_VERSION,
        playlist_id: playlist.id.clone(),
        name: playlist.name.clone(),
        collage_seed: playlist.collage_seed,
        created_at: playlist.created_at.clone(),
        updated_at: Utc::now().to_rfc3339(),
        tracks,
    }
}

/// Builds an M3U8 string with relative paths for a remote playlist located in `playlists/`.
pub fn build_remote_playlist_m3u8(manifest: &RemotePlaylistManifest) -> String {
    let mut lines = vec!["#EXTM3U".to_string()];
    for track in &manifest.tracks {
        let duration_secs = track.duration_ms / 1000;
        let display = match (&track.composer, &track.title) {
            (Some(c), t) if !c.trim().is_empty() => format!("{} - {}", c.trim(), t.trim()),
            _ => track.title.trim().to_string(),
        };
        lines.push(format!("#EXTINF:{duration_secs},{display}"));
        lines.push(format!("../{}", track.relative_path.trim_start_matches('/')));
    }
    lines.join("\r\n")
}

/// Builds a `MasterPlaylistIndex` from a list of playlist summaries.
pub fn build_master_playlist_index(playlists: Vec<RemotePlaylistSummary>) -> MasterPlaylistIndex {
    MasterPlaylistIndex {
        manifest_version: CURRENT_MANIFEST_VERSION,
        updated_at: Utc::now().to_rfc3339(),
        playlists,
    }
}

pub fn serialize_playlist_manifest(manifest: &RemotePlaylistManifest) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(manifest)
}

pub fn deserialize_playlist_manifest(json: &str) -> Result<RemotePlaylistManifest, serde_json::Error> {
    serde_json::from_str(json)
}

pub fn serialize_master_playlist_index(index: &MasterPlaylistIndex) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(index)
}

pub fn deserialize_master_playlist_index(json: &str) -> Result<MasterPlaylistIndex, serde_json::Error> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_domain::AudioPropertiesSnapshot;
    use std::collections::BTreeMap;

    #[test]
    fn test_sanitize_path_segment() {
        assert_eq!(
            sanitize_path_segment("Symphony No. 9 in D minor, Op. 125: Choral"),
            "Symphony No. 9 in D minor, Op. 125_ Choral"
        );
        assert_eq!(sanitize_path_segment("  ...test/folder...  "), "test_folder");
        assert_eq!(sanitize_path_segment(""), "Unknown");
    }

    #[test]
    fn test_disc_directory_and_album_id() {
        assert!(is_disc_directory_name("Disc 1"));
        assert!(is_disc_directory_name("Disc 02"));
        assert!(is_disc_directory_name("disc 10"));
        assert!(is_disc_directory_name("CD 1"));
        assert!(is_disc_directory_name("CD2"));
        assert!(is_disc_directory_name("Disk 3"));
        assert!(is_disc_directory_name("Vol. 1"));
        assert!(is_disc_directory_name("Volume 2"));
        assert!(is_disc_directory_name("Part 1"));
        assert!(!is_disc_directory_name("Discovery"));
        assert!(!is_disc_directory_name("Discus"));
        assert!(!is_disc_directory_name("Partita No. 1"));
        assert!(!is_disc_directory_name("Volume"));

        let single_disc_path = "G:\\Music\\Vivaldi - Four Seasons (1994)\\01.flac";
        assert_eq!(
            effective_album_directory(single_disc_path),
            "G:\\Music\\Vivaldi - Four Seasons (1994)"
        );

        let multi_disc1 = "G:\\Music\\Bach - Mass\\Disc 1\\01.flac";
        let multi_disc2 = "G:\\Music\\Bach - Mass\\Disc 2\\01.flac";
        assert_eq!(
            effective_album_directory(multi_disc1),
            "G:\\Music\\Bach - Mass"
        );
        assert_eq!(
            effective_album_directory(multi_disc2),
            "G:\\Music\\Bach - Mass"
        );

        let mut t1 = ScannedTrack::default();
        t1.path = multi_disc1.to_string();
        t1.mapped_fields.insert("album".into(), vec!["Mass in B Minor".into()]);

        let mut t2 = ScannedTrack::default();
        t2.path = multi_disc2.to_string();
        t2.mapped_fields.insert("album".into(), vec!["Mass in B Minor".into()]);

        assert_eq!(album_id_for_track(&t1), album_id_for_track(&t2));
        assert_eq!(
            album_id_for_track(&t1),
            "mass in b minor::g:/music/bach - mass"
        );

        // Different recordings with same album name get different IDs
        let mut t3_other_recording = ScannedTrack::default();
        t3_other_recording.path = "G:\\Music\\Gardiner - Mass\\01.flac".to_string();
        t3_other_recording.mapped_fields.insert("album".into(), vec!["Mass in B Minor".into()]);
        assert_ne!(album_id_for_track(&t1), album_id_for_track(&t3_other_recording));
    }

    #[test]
    fn test_deterministic_album_and_track_id() {
        let composers = vec!["Ludwig van Beethoven".to_string()];
        let catalogs = vec!["Op. 125".to_string()];

        let id1 = derive_canonical_album_id("Symphony No. 9", &composers, &catalogs);
        let id2 = derive_canonical_album_id("  symphony no. 9  ", &composers, &catalogs);
        assert_eq!(id1, id2);
        assert!(id1.starts_with("alb_"));

        let trk1 = derive_canonical_track_id(&id1, Some("1"), Some("1"), "Allegro", "01.flac");
        let trk2 = derive_canonical_track_id(&id1, Some("1"), Some("1"), "allegro", "01.flac");
        assert_eq!(trk1, trk2);
        assert!(trk1.starts_with("trk_"));
    }

    #[test]
    fn test_derive_systematic_file_name() {
        let name1 = derive_systematic_file_name(Some("1"), Some("4"), "Finale: Ode to Joy", "orig.flac");
        assert_eq!(name1, "04 - Finale_ Ode to Joy.flac");

        let name2 = derive_systematic_file_name(Some("2"), Some("1"), "Act II", "orig.flac");
        assert_eq!(name2, "2-01 - Act II.flac");
    }

    #[test]
    fn test_manifest_round_trip_serialization() {
        let mut mapped = BTreeMap::new();
        mapped.insert("composer".to_string(), vec!["J.S. Bach".to_string()]);
        mapped.insert("title".to_string(), vec!["Prelude in C Major".to_string()]);
        mapped.insert("track_number".to_string(), vec!["1".to_string()]);

        let track = ScannedTrack {
            id: "local_id".to_string(),
            path: "local_path.flac".to_string(),
            file_name: "prelude.flac".to_string(),
            album_art_path: None,
            audio: AudioPropertiesSnapshot {
                format: "FLAC".to_string(),
                duration_ms: 120_000,
                sample_rate: Some(96000),
                bit_depth: Some(24),
                channels: Some(2),
            },
            raw_tags: BTreeMap::new(),
            mapped_fields: mapped,
        };

        let manifest = build_album_manifest(
            "Well-Tempered Clavier",
            &[track],
            Some("cover.jpg".to_string()),
            false,
        )
        .expect("build manifest");

        assert_eq!(manifest.composers, vec!["J.S. Bach".to_string()]);
        assert_eq!(manifest.tracks.len(), 1);
        assert_eq!(manifest.tracks[0].relative_file_name, "01 - Prelude in C Major.flac");

        let json = serialize_manifest(&manifest).expect("serialize");
        let deserialized = deserialize_manifest(&json).expect("deserialize");
        assert_eq!(manifest, deserialized);

        let folder = derive_systematic_folder_name(&manifest);
        assert_eq!(folder, "J.S. Bach/Well-Tempered Clavier");

        let summary = build_master_album_summary(&manifest, &folder);
        assert_eq!(summary.track_count, 1);
        assert_eq!(summary.total_duration_ms, 120_000);
        assert_eq!(summary.updated_at, Some(manifest.updated_at.clone()));

        let master_index = build_master_library_index(vec![summary]);
        let index_json = serialize_master_index(&master_index).expect("serialize master");
        let deserialized_index = deserialize_master_index(&index_json).expect("deserialize master");
        assert_eq!(master_index, deserialized_index);
    }

    #[test]
    fn test_remote_playlist_manifest_and_m3u8_roundtrip() {
        let playlist = Playlist {
            id: "my-favorites-123".to_string(),
            name: "Favorites".to_string(),
            collage_seed: 42,
            track_ids: vec!["trk_1".to_string()],
            created_at: Some("2026-10-21T10:00:00Z".to_string()),
        };

        let tracks = vec![RemotePlaylistTrack {
            track_id: "trk_1".to_string(),
            relative_path: "J.S. Bach/Well-Tempered Clavier/01 - Prelude.flac".to_string(),
            title: "Prelude in C Major".to_string(),
            composer: Some("J.S. Bach".to_string()),
            album: Some("Well-Tempered Clavier".to_string()),
            duration_ms: 120_000,
        }];

        let manifest = build_remote_playlist_manifest(&playlist, tracks);
        assert_eq!(manifest.playlist_id, "my-favorites-123");
        assert_eq!(manifest.name, "Favorites");
        assert_eq!(manifest.tracks.len(), 1);

        let json = serialize_playlist_manifest(&manifest).expect("serialize playlist manifest");
        let deserialized = deserialize_playlist_manifest(&json).expect("deserialize playlist manifest");
        assert_eq!(manifest, deserialized);

        let m3u8 = build_remote_playlist_m3u8(&manifest);
        assert!(m3u8.starts_with("#EXTM3U\r\n#EXTINF:120,J.S. Bach - Prelude in C Major\r\n../J.S. Bach/Well-Tempered Clavier/01 - Prelude.flac"));

        let summary = RemotePlaylistSummary {
            playlist_id: manifest.playlist_id.clone(),
            name: manifest.name.clone(),
            track_count: manifest.tracks.len(),
            updated_at: manifest.updated_at.clone(),
            relative_path: format!("playlists/{}.json", manifest.playlist_id),
            m3u_path: Some("playlists/Favorites.m3u8".to_string()),
        };

        let index = build_master_playlist_index(vec![summary]);
        let index_json = serialize_master_playlist_index(&index).expect("serialize playlist index");
        let deserialized_index = deserialize_master_playlist_index(&index_json).expect("deserialize playlist index");
        assert_eq!(index, deserialized_index);
    }

    #[test]
    fn test_album_manifest_and_folder_differentiates_performances() {
        let m1 = AlbumManifest {
            manifest_version: CURRENT_MANIFEST_VERSION,
            album_id: "alb_1".into(),
            title: "The Four Seasons".into(),
            composers: vec!["Antonio Vivaldi".into()],
            conductors: vec!["Giovanni Antonini".into()],
            ensembles: vec!["Il Giardino Armonico".into()],
            soloists: vec![],
            catalog_numbers: vec![],
            year: Some("1993".into()),
            artwork_file_name: None,
            updated_at: "2026-10-30T00:00:00Z".into(),
            tracks: vec![],
        };

        let m2 = AlbumManifest {
            manifest_version: CURRENT_MANIFEST_VERSION,
            album_id: "alb_2".into(),
            title: "The Four Seasons".into(),
            composers: vec!["Antonio Vivaldi".into()],
            conductors: vec!["Seiji Ozawa".into()],
            ensembles: vec!["Boston Symphony Orchestra".into()],
            soloists: vec!["Itzhak Perlman".into()],
            catalog_numbers: vec![],
            year: Some("1981".into()),
            artwork_file_name: None,
            updated_at: "2026-10-30T00:00:00Z".into(),
            tracks: vec![],
        };

        let f1 = derive_systematic_folder_name(&m1);
        let f2 = derive_systematic_folder_name(&m2);
        assert_ne!(f1, f2);
        assert_eq!(f1, "Antonio Vivaldi/The Four Seasons (Giovanni Antonini, Il Giardino Armonico - 1993)");
        assert_eq!(f2, "Antonio Vivaldi/The Four Seasons (Seiji Ozawa, Boston Symphony Orchestra - 1981)");

        let id1 = derive_canonical_album_id_full(
            &m1.title,
            &m1.composers,
            &m1.conductors,
            &m1.ensembles,
            &m1.soloists,
            m1.year.as_deref(),
            &m1.catalog_numbers,
        );
        let id2 = derive_canonical_album_id_full(
            &m2.title,
            &m2.composers,
            &m2.conductors,
            &m2.ensembles,
            &m2.soloists,
            m2.year.as_deref(),
            &m2.catalog_numbers,
        );
        assert_ne!(id1, id2, "Canonical album IDs must differ for different recordings");
    }
}
