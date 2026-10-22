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

/// Derives a deterministic canonical album ID based on album title, composers, and catalog numbers.
pub fn derive_canonical_album_id(
    title: &str,
    composers: &[String],
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

    let album_id = derive_canonical_album_id(album_title, &composers, &catalog_numbers);

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

/// Derives the systematic folder path for an album: e.g., "Composer/Album".
pub fn derive_systematic_folder_name(manifest: &AlbumManifest) -> String {
    let composer = manifest
        .composers
        .first()
        .map(|s| sanitize_path_segment(s))
        .unwrap_or_else(|| "Various Composers".to_string());

    let album = sanitize_path_segment(&manifest.title);
    format!("{}/{}", composer, album)
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
}
