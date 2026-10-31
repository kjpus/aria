use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use futures_util::stream::{self, StreamExt};

use aria_domain::{
    AlbumManifest, ManifestTrack, MasterAlbumSummary, Playlist, RemotePlaylistManifest,
    RemotePlaylistSummary, RemotePlaylistTrack, RemoteSyncProgressEvent, RemoteSyncedItem,
    RemoteTarget, ScannedTrack, UploadFailedItem, UploadProgressEvent,
};
use aria_library::manifest::{
    build_album_manifest, build_master_album_summary, build_master_library_index,
    build_master_playlist_index, build_remote_playlist_manifest, build_remote_playlist_m3u8,
    compute_file_sha256, derive_systematic_folder_name, deserialize_manifest,
    deserialize_master_index, deserialize_master_playlist_index, deserialize_playlist_manifest,
    sanitize_path_segment, serialize_manifest, serialize_master_index,
    serialize_master_playlist_index, serialize_playlist_manifest,
};
use chrono::Utc;
use tokio::sync::broadcast;

use crate::error::RemoteStorageError;
use crate::quota::ensure_within_quota;
use crate::traits::RemoteStorageBackend;

pub struct UploadOptions {
    pub compute_checksums: bool,
    pub current_target_usage_bytes: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UploadAlbumResult {
    pub synced_items: Vec<RemoteSyncedItem>,
    pub failed_items: Vec<UploadFailedItem>,
    pub new_tracks_uploaded: usize,
    pub already_synced_tracks: usize,
}

/// Executes the full upload pipeline for an album to a remote target.
pub async fn upload_album(
    backend: Arc<dyn RemoteStorageBackend>,
    target: &RemoteTarget,
    album_title: &str,
    tracks: &[ScannedTrack],
    options: UploadOptions,
    progress_tx: Option<broadcast::Sender<UploadProgressEvent>>,
) -> Result<UploadAlbumResult, RemoteStorageError> {
    if tracks.is_empty() {
        return Err(RemoteStorageError::ProviderError("No tracks in album".into()));
    }

    // 1. Discover cover artwork
    let artwork_path = tracks.iter().find_map(|t| {
        t.album_art_path.as_ref().and_then(|p| {
            if Path::new(p).is_file() {
                Some(p.clone())
            } else {
                None
            }
        })
    });
    let artwork_file_name = if artwork_path.is_some() {
        Some("cover.jpg".to_string())
    } else {
        None
    };

    // 2. Build initial album manifest for current tracks
    let mut manifest: AlbumManifest = build_album_manifest(
        album_title,
        tracks,
        artwork_file_name.clone(),
        options.compute_checksums,
    )
    .map_err(RemoteStorageError::Io)?;

    let relative_folder = derive_systematic_folder_name(&manifest);

    // Emit initial progress event so UI immediately shows active upload
    let initial_total_files = manifest.tracks.len() + if artwork_path.is_some() { 1 } else { 0 } + 1;
    emit_progress(
        progress_tx.as_ref(),
        target,
        &manifest,
        "aria-manifest.json",
        0,
        0,
        initial_total_files,
        0,
        0,
        0,
        "checking_remote",
    );

    // 3. Check if album already exists on remote target
    let remote_manifest_path = format!("{relative_folder}/aria-manifest.json");
    let existing_manifest: Option<AlbumManifest> = match backend.read_bytes(&remote_manifest_path).await {
        Ok(bytes) => {
            let json = String::from_utf8_lossy(&bytes);
            deserialize_manifest(&json).ok()
        }
        Err(_) => None,
    };

    // 4. Check cover art: does it already exist on remote with identical content?
    let mut cover_needs_upload = artwork_path.is_some();
    let mut art_data_cache: Option<Vec<u8>> = None;
    if let Some(ref art_path) = artwork_path {
        if let Ok(art_data) = tokio::fs::read(art_path).await {
            let remote_cover_path = format!("{relative_folder}/cover.jpg");
            if existing_manifest.as_ref().and_then(|m| m.artwork_file_name.as_ref()).is_some() {
                if let Ok(existing_bytes) = backend.read_bytes(&remote_cover_path).await {
                    if existing_bytes == art_data {
                        cover_needs_upload = false;
                    }
                }
            }
            art_data_cache = Some(art_data);
        }
    }

    // 5. Check each track: has it already been uploaded with the same content?
    let mut tracks_need_upload = Vec::with_capacity(tracks.len());
    let mut resolved_sizes = Vec::with_capacity(tracks.len());

    for (track, manifest_track) in tracks.iter().zip(manifest.tracks.iter()) {
        let is_from_target = track.path.starts_with(&format!("remote://{}/", target.id));
        let mut needs_upload = true;
        let mut track_size = manifest_track.file_size_bytes;

        if is_from_target {
            needs_upload = false;
        } else if let Some(ref ex) = existing_manifest {
            if let Some(matching) = ex.tracks.iter().find(|t| {
                t.track_id == manifest_track.track_id || t.relative_file_name == manifest_track.relative_file_name
            }) {
                let local_path = Path::new(&track.path);
                let local_size = manifest_track.file_size_bytes;
                let size_matches = local_size > 0 && local_size == matching.file_size_bytes;
                let duration_matches = matching.audio.duration_ms == 0
                    || manifest_track.audio.duration_ms == 0
                    || matching.audio.duration_ms == manifest_track.audio.duration_ms;

                if size_matches && duration_matches {
                    let sha_matches = match (&matching.audio_checksum_sha256, &manifest_track.audio_checksum_sha256) {
                        (Some(remote_sha), Some(local_sha)) => local_sha.eq_ignore_ascii_case(remote_sha),
                        (Some(remote_sha), None) if options.compute_checksums => {
                            if local_path.is_file() {
                                compute_file_sha256(local_path).map(|s| s.eq_ignore_ascii_case(remote_sha)).unwrap_or(false)
                            } else {
                                false
                            }
                        }
                        _ => true,
                    };

                    let audio_props_match = matching.audio.sample_rate == manifest_track.audio.sample_rate
                        && matching.audio.channels == manifest_track.audio.channels;

                    if sha_matches && audio_props_match {
                        needs_upload = false;
                    }
                }

                if !needs_upload && track_size == 0 {
                    track_size = matching.file_size_bytes;
                }
            }
        }

        tracks_need_upload.push(needs_upload);
        resolved_sizes.push(track_size);
    }

    // 6. Pre-flight quota check: only check new/changed bytes so already-uploaded tracks don't trigger quota errors
    let total_bytes: u64 = resolved_sizes.iter().sum();
    let mut new_bytes_to_upload = 0u64;
    for (idx, &needs_upload) in tracks_need_upload.iter().enumerate() {
        if needs_upload {
            new_bytes_to_upload += resolved_sizes[idx];
        }
    }
    if cover_needs_upload {
        if let Some(ref data) = art_data_cache {
            new_bytes_to_upload += data.len() as u64;
        }
    }

    if new_bytes_to_upload > 0 {
        ensure_within_quota(
            options.current_target_usage_bytes,
            new_bytes_to_upload + 100_000,
            target.storage_limit_bytes,
        )?;
    }

    let total_files = manifest.tracks.len() + if artwork_path.is_some() { 1 } else { 0 } + 1;
    let mut completed_files = 0usize;
    let mut completed_bytes = 0u64;

    // Fast-path: if all tracks and cover are already on remote and manifest is current, skip uploading
    let any_new_tracks_for_manifest = match existing_manifest {
        None => !manifest.tracks.is_empty(),
        Some(ref ex) => manifest.tracks.iter().any(|mt| {
            !ex.tracks.iter().any(|et| et.track_id == mt.track_id || et.relative_file_name == mt.relative_file_name)
        }),
    };

    let all_already_synced = !cover_needs_upload
        && tracks_need_upload.iter().all(|&needs| !needs)
        && existing_manifest.is_some()
        && !any_new_tracks_for_manifest;

    if all_already_synced {
        let now = Utc::now().to_rfc3339();
        let mut synced_items = Vec::with_capacity(manifest.tracks.len() + 1);
        for (idx, manifest_track) in manifest.tracks.iter().enumerate() {
            let remote_file_path = format!("{relative_folder}/{}", manifest_track.relative_file_name);
            let track_size = resolved_sizes[idx];
            synced_items.push(RemoteSyncedItem {
                item_id: manifest_track.track_id.clone(),
                remote_target_id: target.id.clone(),
                item_type: "track".to_string(),
                remote_path: remote_file_path,
                remote_file_id: None,
                size_bytes: track_size,
                checksum: manifest_track.audio_checksum_sha256.clone(),
                sync_status: "synced".to_string(),
                last_synced_at: now.clone(),
            });
        }
        let total_album_size = manifest.tracks.iter().map(|t| t.file_size_bytes).sum();
        synced_items.push(RemoteSyncedItem {
            item_id: manifest.album_id.clone(),
            remote_target_id: target.id.clone(),
            item_type: "album".to_string(),
            remote_path: relative_folder,
            remote_file_id: None,
            size_bytes: total_album_size,
            checksum: None,
            sync_status: "synced".to_string(),
            last_synced_at: now,
        });

        emit_progress(
            progress_tx.as_ref(),
            target,
            &manifest,
            "Done",
            total_bytes,
            total_bytes,
            total_files,
            total_files,
            total_bytes,
            total_bytes,
            "already_synced",
        );

        return Ok(UploadAlbumResult {
            synced_items,
            failed_items: Vec::new(),
            new_tracks_uploaded: 0,
            already_synced_tracks: tracks.len(),
        });
    }

    // 7. Ensure remote album directory exists
    if let Err(e) = backend.ensure_directory(&relative_folder).await {
        let mut failed_items = Vec::new();
        for track in tracks {
            let title = track
                .mapped_fields
                .get("title")
                .and_then(|t| t.first())
                .cloned()
                .unwrap_or_else(|| track.file_name.clone());
            failed_items.push(UploadFailedItem {
                item_id: Some(track.id.clone()),
                title,
                path: track.path.clone(),
                error: format!("Failed to create folder '{}': {}", relative_folder, e),
            });
        }
        return Ok(UploadAlbumResult {
            synced_items: Vec::new(),
            failed_items,
            new_tracks_uploaded: 0,
            already_synced_tracks: 0,
        });
    }

    // 8. Upload cover art if needed
    if let Some(art_path) = artwork_path {
        if cover_needs_upload {
            emit_progress(
                progress_tx.as_ref(),
                target,
                &manifest,
                "cover.jpg",
                0,
                0,
                total_files,
                completed_files,
                total_bytes,
                completed_bytes,
                "uploading_art",
            );
            let remote_cover_path = format!("{relative_folder}/cover.jpg");
            let art_data = if let Some(cached) = art_data_cache {
                Some(cached)
            } else {
                tokio::fs::read(&art_path).await.ok()
            };
            if let Some(data) = art_data {
                if let Err(e) = backend.write_bytes(&remote_cover_path, &data).await {
                    eprintln!("Warning: failed to upload cover art: {e}");
                }
            }
        } else {
            emit_progress(
                progress_tx.as_ref(),
                target,
                &manifest,
                "cover.jpg",
                0,
                0,
                total_files,
                completed_files,
                total_bytes,
                completed_bytes,
                "already_synced",
            );
        }
        completed_files += 1;
    }

    // 9. Upload audio files (skipping unchanged tracks, recording failures without aborting)
    let mut synced_items = Vec::new();
    let mut failed_items = Vec::new();
    let mut successful_manifest_track_ids = HashSet::new();
    let mut new_tracks_uploaded = 0usize;
    let mut already_synced_tracks = 0usize;
    let now = Utc::now().to_rfc3339();

    for (idx, (track, manifest_track)) in tracks.iter().zip(manifest.tracks.iter()).enumerate() {
        let local_path = Path::new(&track.path);
        let remote_file_path = format!("{relative_folder}/{}", manifest_track.relative_file_name);
        let track_size = resolved_sizes[idx];
        let needs_upload = tracks_need_upload[idx];

        let mut remote_file_id: Option<String> = None;

        if needs_upload {
            emit_progress(
                progress_tx.as_ref(),
                target,
                &manifest,
                &manifest_track.relative_file_name,
                0,
                track_size,
                total_files,
                completed_files,
                total_bytes,
                completed_bytes,
                "uploading_track",
            );

            match backend.upload_file(local_path, &remote_file_path, None).await {
                Ok(file_ref) => {
                    remote_file_id = Some(file_ref.remote_id);
                    completed_bytes += track_size;
                    completed_files += 1;
                    new_tracks_uploaded += 1;

                    emit_progress(
                        progress_tx.as_ref(),
                        target,
                        &manifest,
                        &manifest_track.relative_file_name,
                        track_size,
                        track_size,
                        total_files,
                        completed_files,
                        total_bytes,
                        completed_bytes,
                        "track_uploaded",
                    );

                    successful_manifest_track_ids.insert(manifest_track.track_id.clone());
                    synced_items.push(RemoteSyncedItem {
                        item_id: manifest_track.track_id.clone(),
                        remote_target_id: target.id.clone(),
                        item_type: "track".to_string(),
                        remote_path: remote_file_path,
                        remote_file_id,
                        size_bytes: track_size,
                        checksum: manifest_track.audio_checksum_sha256.clone(),
                        sync_status: "synced".to_string(),
                        last_synced_at: now.clone(),
                    });
                }
                Err(e) => {
                    completed_files += 1;
                    let title = track
                        .mapped_fields
                        .get("title")
                        .and_then(|t| t.first())
                        .cloned()
                        .unwrap_or_else(|| track.file_name.clone());
                    failed_items.push(UploadFailedItem {
                        item_id: Some(manifest_track.track_id.clone()),
                        title,
                        path: track.path.clone(),
                        error: e.to_string(),
                    });

                    emit_progress(
                        progress_tx.as_ref(),
                        target,
                        &manifest,
                        &manifest_track.relative_file_name,
                        0,
                        track_size,
                        total_files,
                        completed_files,
                        total_bytes,
                        completed_bytes,
                        "track_failed",
                    );
                }
            }
        } else {
            completed_bytes += track_size;
            completed_files += 1;
            already_synced_tracks += 1;

            emit_progress(
                progress_tx.as_ref(),
                target,
                &manifest,
                &manifest_track.relative_file_name,
                track_size,
                track_size,
                total_files,
                completed_files,
                total_bytes,
                completed_bytes,
                "already_synced",
            );

            successful_manifest_track_ids.insert(manifest_track.track_id.clone());
            synced_items.push(RemoteSyncedItem {
                item_id: manifest_track.track_id.clone(),
                remote_target_id: target.id.clone(),
                item_type: "track".to_string(),
                remote_path: remote_file_path,
                remote_file_id,
                size_bytes: track_size,
                checksum: manifest_track.audio_checksum_sha256.clone(),
                sync_status: "synced".to_string(),
                last_synced_at: now.clone(),
            });
        }
    }

    // Only keep tracks in this run that actually succeeded or were already synced
    manifest.tracks.retain(|t| successful_manifest_track_ids.contains(&t.track_id));

    // 10. Merge with existing remote manifest to preserve previously-uploaded tracks from other playlists/syncs
    let any_files_uploaded = cover_needs_upload || tracks_need_upload.iter().any(|&b| b);
    let any_new_tracks_for_manifest = match existing_manifest {
        None => !manifest.tracks.is_empty(),
        Some(ref ex) => manifest.tracks.iter().any(|mt| {
            !ex.tracks.iter().any(|et| et.track_id == mt.track_id || et.relative_file_name == mt.relative_file_name)
        }),
    };
    let any_tracks_remotely = !manifest.tracks.is_empty()
        || existing_manifest.as_ref().map(|ex| !ex.tracks.is_empty()).unwrap_or(false);

    let should_update_manifest = any_tracks_remotely && (existing_manifest.is_none() || any_files_uploaded || any_new_tracks_for_manifest);

    if let Some(ref ex) = existing_manifest {
        if ex.album_id == manifest.album_id {
            let mut merged_tracks = manifest.tracks.clone();
            for ex_track in &ex.tracks {
                let already_present = merged_tracks.iter().any(|t| {
                    t.track_id == ex_track.track_id || t.relative_file_name == ex_track.relative_file_name
                });
                if !already_present {
                    merged_tracks.push(ex_track.clone());
                }
            }

            let get_track_sort_key = |t: &ManifestTrack| -> (u32, u32, String) {
                let disk = t
                    .mapped_fields
                    .get("disk_number")
                    .and_then(|v| v.first())
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(1);
                let track = t
                    .mapped_fields
                    .get("track_number")
                    .and_then(|v| v.first())
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(0);
                (disk, track, t.relative_file_name.clone())
            };
            merged_tracks.sort_by(|a, b| get_track_sort_key(a).cmp(&get_track_sort_key(b)));
            manifest.tracks = merged_tracks;

            if manifest.artwork_file_name.is_none() {
                manifest.artwork_file_name = ex.artwork_file_name.clone();
            }
            for c in &ex.composers {
                if !manifest.composers.contains(c) {
                    manifest.composers.push(c.clone());
                }
            }
            for c in &ex.conductors {
                if !manifest.conductors.contains(c) {
                    manifest.conductors.push(c.clone());
                }
            }
            for e in &ex.ensembles {
                if !manifest.ensembles.contains(e) {
                    manifest.ensembles.push(e.clone());
                }
            }
            for s in &ex.soloists {
                if !manifest.soloists.contains(s) {
                    manifest.soloists.push(s.clone());
                }
            }
            for cat in &ex.catalog_numbers {
                if !manifest.catalog_numbers.contains(cat) {
                    manifest.catalog_numbers.push(cat.clone());
                }
            }
            if manifest.year.is_none() && ex.year.is_some() {
                manifest.year = ex.year.clone();
            }
        }
    }

    if should_update_manifest {
        emit_progress(
            progress_tx.as_ref(),
            target,
            &manifest,
            "aria-manifest.json",
            0,
            0,
            total_files,
            completed_files,
            total_bytes,
            completed_bytes,
            "writing_manifest",
        );
        manifest.updated_at = now.clone();
        if let Ok(manifest_json) = serialize_manifest(&manifest) {
            if let Err(e) = backend
                .write_bytes(&remote_manifest_path, manifest_json.as_bytes())
                .await
            {
                failed_items.push(UploadFailedItem {
                    item_id: Some(manifest.album_id.clone()),
                    title: manifest.title.clone(),
                    path: remote_manifest_path.clone(),
                    error: format!("Failed to write manifest: {e}"),
                });
            }
        }

        emit_progress(
            progress_tx.as_ref(),
            target,
            &manifest,
            "aria-library-index.json",
            0,
            0,
            total_files,
            completed_files,
            total_bytes,
            completed_bytes,
            "updating_master_index",
        );
        let mut master_index = match backend.read_bytes("aria-library-index.json").await {
            Ok(bytes) => {
                let json = String::from_utf8_lossy(&bytes);
                deserialize_master_index(&json).unwrap_or_else(|_| build_master_library_index(Vec::new()))
            }
            Err(_) => build_master_library_index(Vec::new()),
        };

        let summary = build_master_album_summary(&manifest, &relative_folder);
        master_index.albums.retain(|a| a.album_id != manifest.album_id);
        master_index.albums.push(summary);
        master_index.updated_at = now.clone();

        if let Ok(master_json) = serialize_master_index(&master_index) {
            if let Err(e) = backend
                .write_bytes("aria-library-index.json", master_json.as_bytes())
                .await
            {
                failed_items.push(UploadFailedItem {
                    item_id: None,
                    title: "Master Library Index".to_string(),
                    path: "aria-library-index.json".to_string(),
                    error: format!("Failed to update master index: {e}"),
                });
            }
        }
    }

    // 11. Create album synced item if any tracks exist remotely
    if any_tracks_remotely {
        let total_album_size = manifest.tracks.iter().map(|t| t.file_size_bytes).sum();
        synced_items.push(RemoteSyncedItem {
            item_id: manifest.album_id.clone(),
            remote_target_id: target.id.clone(),
            item_type: "album".to_string(),
            remote_path: relative_folder,
            remote_file_id: None,
            size_bytes: total_album_size,
            checksum: None,
            sync_status: "synced".to_string(),
            last_synced_at: now,
        });
    }

    emit_progress(
        progress_tx.as_ref(),
        target,
        &manifest,
        "Done",
        total_bytes,
        total_bytes,
        total_files,
        total_files,
        total_bytes,
        total_bytes,
        "completed",
    );

    Ok(UploadAlbumResult {
        synced_items,
        failed_items,
        new_tracks_uploaded,
        already_synced_tracks,
    })
}

fn emit_progress(
    progress_tx: Option<&broadcast::Sender<UploadProgressEvent>>,
    target: &RemoteTarget,
    manifest: &AlbumManifest,
    current_file: &str,
    current_file_bytes: u64,
    current_file_total_bytes: u64,
    total_files: usize,
    completed_files: usize,
    total_bytes: u64,
    completed_bytes: u64,
    status: &str,
) {
    if let Some(tx) = progress_tx {
        let _ = tx.send(UploadProgressEvent {
            target_id: target.id.clone(),
            album_id: manifest.album_id.clone(),
            album_title: manifest.title.clone(),
            current_file: current_file.to_string(),
            current_file_bytes,
            current_file_total_bytes,
            total_files,
            completed_files,
            total_bytes,
            completed_bytes,
            status: status.to_string(),
        });
    }
}

/// Uploads a playlist manifest (.json) and portable playlist (.m3u8) to the remote target,
/// and updates aria-playlists-index.json at the target root.
pub async fn upload_playlist(
    backend: Arc<dyn RemoteStorageBackend>,
    target: &RemoteTarget,
    playlist: &Playlist,
    playlist_tracks: &[RemotePlaylistTrack],
    progress_tx: Option<broadcast::Sender<UploadProgressEvent>>,
) -> Result<RemoteSyncedItem, RemoteStorageError> {
    if let Some(ref tx) = progress_tx {
        let _ = tx.send(UploadProgressEvent {
            target_id: target.id.clone(),
            album_id: playlist.id.clone(),
            album_title: playlist.name.clone(),
            current_file: "playlists/manifest".to_string(),
            current_file_bytes: 0,
            current_file_total_bytes: 0,
            total_files: 3,
            completed_files: 0,
            total_bytes: 0,
            completed_bytes: 0,
            status: "uploading_playlist_manifest".to_string(),
        });
    }

    // 1. Ensure playlists directory exists
    backend.ensure_directory("playlists").await?;

    // 2. Build RemotePlaylistManifest
    let mut playlist_manifest = build_remote_playlist_manifest(playlist, playlist_tracks.to_vec());
    let manifest_remote_path = format!("playlists/{}.json", playlist.id);
    let clean_name = sanitize_path_segment(&playlist.name);
    let m3u8_remote_path = format!("playlists/{clean_name}.m3u8");

    let existing_manifest = match backend.read_bytes(&manifest_remote_path).await {
        Ok(bytes) => {
            let json = String::from_utf8_lossy(&bytes);
            deserialize_playlist_manifest(&json).ok()
        }
        Err(_) => None,
    };

    let is_identical = existing_manifest.as_ref().map(|ex| {
        ex.name == playlist.name
            && ex.collage_seed == playlist.collage_seed
            && ex.tracks == playlist_tracks
    }).unwrap_or(false);

    let (manifest_json, m3u8_content) = if is_identical {
        let ex = existing_manifest.as_ref().unwrap();
        playlist_manifest.updated_at = ex.updated_at.clone();
        let m_json = serialize_playlist_manifest(&playlist_manifest)
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;
        let m3u8 = build_remote_playlist_m3u8(&playlist_manifest);
        (m_json, m3u8)
    } else {
        let m_json = serialize_playlist_manifest(&playlist_manifest)
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;
        backend
            .write_bytes(&manifest_remote_path, m_json.as_bytes())
            .await?;

        if let Some(ref tx) = progress_tx {
            let _ = tx.send(UploadProgressEvent {
                target_id: target.id.clone(),
                album_id: playlist.id.clone(),
                album_title: playlist.name.clone(),
                current_file: format!("playlists/{}.json", playlist.id),
                current_file_bytes: m_json.len() as u64,
                current_file_total_bytes: m_json.len() as u64,
                total_files: 3,
                completed_files: 1,
                total_bytes: m_json.len() as u64,
                completed_bytes: m_json.len() as u64,
                status: "uploading_playlist_m3u8".to_string(),
            });
        }

        // 3. Build and write M3U8
        let m3u8 = build_remote_playlist_m3u8(&playlist_manifest);
        backend
            .write_bytes(&m3u8_remote_path, m3u8.as_bytes())
            .await?;

        if let Some(ref tx) = progress_tx {
            let _ = tx.send(UploadProgressEvent {
                target_id: target.id.clone(),
                album_id: playlist.id.clone(),
                album_title: playlist.name.clone(),
                current_file: m3u8_remote_path.clone(),
                current_file_bytes: m3u8.len() as u64,
                current_file_total_bytes: m3u8.len() as u64,
                total_files: 3,
                completed_files: 2,
                total_bytes: (m_json.len() + m3u8.len()) as u64,
                completed_bytes: (m_json.len() + m3u8.len()) as u64,
                status: "updating_playlist_index".to_string(),
            });
        }

        // 4. Update aria-playlists-index.json at target root
        let mut playlists_index = match backend.read_bytes("aria-playlists-index.json").await {
            Ok(bytes) => {
                let json = String::from_utf8_lossy(&bytes);
                deserialize_master_playlist_index(&json)
                    .unwrap_or_else(|_| build_master_playlist_index(Vec::new()))
            }
            Err(_) => build_master_playlist_index(Vec::new()),
        };

        playlists_index.playlists.retain(|p| p.playlist_id != playlist.id);
        playlists_index.playlists.push(RemotePlaylistSummary {
            playlist_id: playlist.id.clone(),
            name: playlist.name.clone(),
            track_count: playlist_manifest.tracks.len(),
            updated_at: playlist_manifest.updated_at.clone(),
            relative_path: manifest_remote_path.clone(),
            m3u_path: Some(m3u8_remote_path),
        });
        playlists_index.updated_at = Utc::now().to_rfc3339();

        let index_json = serialize_master_playlist_index(&playlists_index)
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;
        backend
            .write_bytes("aria-playlists-index.json", index_json.as_bytes())
            .await?;

        (m_json, m3u8)
    };

    let now = Utc::now().to_rfc3339();
    let synced_item = RemoteSyncedItem {
        item_id: playlist.id.clone(),
        remote_target_id: target.id.clone(),
        item_type: "playlist".to_string(),
        remote_path: manifest_remote_path,
        remote_file_id: None,
        size_bytes: manifest_json.len() as u64,
        checksum: None,
        sync_status: "synced".to_string(),
        last_synced_at: now,
    };

    if let Some(ref tx) = progress_tx {
        let _ = tx.send(UploadProgressEvent {
            target_id: target.id.clone(),
            album_id: playlist.id.clone(),
            album_title: playlist.name.clone(),
            current_file: "Done".to_string(),
            current_file_bytes: 0,
            current_file_total_bytes: 0,
            total_files: 3,
            completed_files: 3,
            total_bytes: (manifest_json.len() + m3u8_content.len()) as u64,
            completed_bytes: (manifest_json.len() + m3u8_content.len()) as u64,
            status: "completed".to_string(),
        });
    }

    Ok(synced_item)
}

#[derive(Debug, Clone)]
pub struct SyncRemoteResult {
    pub target_id: String,
    pub target_name: String,
    pub synced_tracks: Vec<ScannedTrack>,
    pub synced_items: Vec<RemoteSyncedItem>,
    pub new_albums_count: usize,
    pub new_tracks_count: usize,
    pub master_index_updated_at: Option<String>,
}

struct FetchedAlbum {
    manifest: AlbumManifest,
    summary: MasterAlbumSummary,
    local_art_path: Option<String>,
}

/// Discovers and synchronizes remote library albums and manifests from a target.
/// Reads aria-library-index.json from the target root and aria-manifest.json for each album.
/// Downloads and caches album cover art into the provided local artwork cache directory.
pub async fn sync_remote_library(
    backend: Arc<dyn RemoteStorageBackend>,
    target: &RemoteTarget,
    artwork_cache_dir: &Path,
    existing_remote_track_ids: &HashSet<String>,
    target_last_synced_at: Option<&str>,
    existing_album_sync_times: &HashMap<String, String>,
    progress_tx: Option<broadcast::Sender<RemoteSyncProgressEvent>>,
) -> Result<SyncRemoteResult, RemoteStorageError> {
    // 1. Read master library index from remote target root
    let index_bytes = match backend.read_bytes("aria-library-index.json").await {
        Ok(bytes) => bytes,
        Err(RemoteStorageError::NotFound(_)) => {
            tracing::info!(
                "No aria-library-index.json found on target '{}', target has no uploaded library yet.",
                target.name
            );
            return Ok(SyncRemoteResult {
                target_id: target.id.clone(),
                target_name: target.name.clone(),
                synced_tracks: Vec::new(),
                synced_items: Vec::new(),
                new_albums_count: 0,
                new_tracks_count: 0,
                master_index_updated_at: None,
            });
        }
        Err(err) => return Err(err),
    };

    let index_str = String::from_utf8_lossy(&index_bytes);
    let master_index = deserialize_master_index(&index_str)
        .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

    let total_albums = master_index.albums.len();

    // Ensure artwork cache directory exists
    let _ = tokio::fs::create_dir_all(artwork_cache_dir).await;

    // Check if any album that declares artwork is missing its cached image on disk
    let has_missing_artwork = master_index.albums.iter().any(|summary| {
        if summary.artwork_file_name.is_some() {
            let clean_album_id: String = summary
                .album_id
                .chars()
                .map(|ch| if ch.is_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
                .collect();
            let target_art_file = artwork_cache_dir.join(format!("{clean_album_id}.jpg"));
            !target_art_file.exists()
        } else {
            false
        }
    });

    // Early exit: only if the master index has not changed since our last sync,
    // local tracks exist, AND all declared artwork files exist locally
    if let Some(last_sync) = target_last_synced_at {
        if last_sync == master_index.updated_at && !existing_remote_track_ids.is_empty() && !has_missing_artwork {
            tracing::info!(
                "Target '{}' master index unchanged and all artwork cached, early exiting.",
                target.name
            );
            if let Some(ref tx) = progress_tx {
                let _ = tx.send(RemoteSyncProgressEvent {
                    target_id: target.id.clone(),
                    target_name: target.name.clone(),
                    current_album: String::new(),
                    completed_albums: total_albums,
                    total_albums,
                    status: "completed".to_string(),
                });
            }
            return Ok(SyncRemoteResult {
                target_id: target.id.clone(),
                target_name: target.name.clone(),
                synced_tracks: Vec::new(),
                synced_items: Vec::new(),
                new_albums_count: 0,
                new_tracks_count: 0,
                master_index_updated_at: Some(master_index.updated_at.clone()),
            });
        }
    }

    // Delta filtering: find which albums need downloading
    let mut albums_to_fetch = Vec::new();
    let mut already_skipped_count = 0;

    for summary in &master_index.albums {
        let can_skip = if existing_remote_track_ids.is_empty() {
            false
        } else if let (Some(synced_at), Some(summary_updated_at)) = (
            existing_album_sync_times.get(&summary.album_id),
            summary.updated_at.as_deref(),
        ) {
            if synced_at.as_str() >= summary_updated_at {
                if summary.artwork_file_name.is_some() {
                    let clean_album_id: String = summary
                        .album_id
                        .chars()
                        .map(|ch| if ch.is_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
                        .collect();
                    let target_art_file = artwork_cache_dir.join(format!("{clean_album_id}.jpg"));
                    target_art_file.exists()
                } else {
                    true
                }
            } else {
                false
            }
        } else {
            false
        };

        if can_skip {
            already_skipped_count += 1;
        } else {
            albums_to_fetch.push(summary.clone());
        }
    }

    if albums_to_fetch.is_empty() {
        if let Some(ref tx) = progress_tx {
            let _ = tx.send(RemoteSyncProgressEvent {
                target_id: target.id.clone(),
                target_name: target.name.clone(),
                current_album: String::new(),
                completed_albums: total_albums,
                total_albums,
                status: "completed".to_string(),
            });
        }
        return Ok(SyncRemoteResult {
            target_id: target.id.clone(),
            target_name: target.name.clone(),
            synced_tracks: Vec::new(),
            synced_items: Vec::new(),
            new_albums_count: 0,
            new_tracks_count: 0,
            master_index_updated_at: Some(master_index.updated_at.clone()),
        });
    }

    let completed_counter = Arc::new(AtomicUsize::new(already_skipped_count));
    const CONCURRENCY_LIMIT: usize = 3;

    let fetched_albums: Vec<Option<FetchedAlbum>> = stream::iter(albums_to_fetch)
        .map(|summary| {
            let backend = backend.clone();
            let artwork_cache_dir = artwork_cache_dir.to_path_buf();
            let target_id = target.id.clone();
            let target_name = target.name.clone();
            let progress_tx = progress_tx.clone();
            let completed_counter = completed_counter.clone();

            async move {
                if let Some(ref tx) = progress_tx {
                    let completed = completed_counter.load(Ordering::Relaxed);
                    let _ = tx.send(RemoteSyncProgressEvent {
                        target_id: target_id.clone(),
                        target_name: target_name.clone(),
                        current_album: summary.title.clone(),
                        completed_albums: completed,
                        total_albums,
                        status: "reading_manifest".to_string(),
                    });
                }

                let rel_folder = summary.relative_folder.trim_matches(['/', '\\']);
                let manifest_path = format!("{rel_folder}/aria-manifest.json");
                let manifest_bytes = match backend.read_bytes(&manifest_path).await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to read manifest for album '{}' from remote path '{manifest_path}': {e}",
                            summary.title
                        );
                        let _ = completed_counter.fetch_add(1, Ordering::Relaxed);
                        return None;
                    }
                };

                let manifest_str = String::from_utf8_lossy(&manifest_bytes);
                let manifest: AlbumManifest = match deserialize_manifest(&manifest_str) {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to parse manifest for album '{}': {e}",
                            summary.title
                        );
                        let _ = completed_counter.fetch_add(1, Ordering::Relaxed);
                        return None;
                    }
                };

                // Download and cache cover art locally if present
                let mut local_art_path: Option<String> = None;
                if let Some(ref art_file_name) = summary.artwork_file_name {
                    let clean_album_id: String = manifest
                        .album_id
                        .chars()
                        .map(|ch| if ch.is_alphanumeric() || ch == '-' || ch == '_' { ch } else { '_' })
                        .collect();
                    let target_art_file = artwork_cache_dir.join(format!("{clean_album_id}.jpg"));

                    if target_art_file.exists() {
                        local_art_path = Some(target_art_file.to_string_lossy().to_string());
                    } else {
                        let remote_art_path = format!("{rel_folder}/{art_file_name}");
                        if let Ok(art_bytes) = backend.read_bytes(&remote_art_path).await {
                            if let Ok(()) = tokio::fs::write(&target_art_file, &art_bytes).await {
                                local_art_path = Some(target_art_file.to_string_lossy().to_string());
                            }
                        }
                    }
                }

                let completed = completed_counter.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref tx) = progress_tx {
                    let _ = tx.send(RemoteSyncProgressEvent {
                        target_id,
                        target_name,
                        current_album: summary.title.clone(),
                        completed_albums: completed,
                        total_albums,
                        status: "reading_manifest".to_string(),
                    });
                }

                Some(FetchedAlbum {
                    manifest,
                    summary,
                    local_art_path,
                })
            }
        })
        .buffer_unordered(CONCURRENCY_LIMIT)
        .collect()
        .await;

    let mut synced_tracks = Vec::new();
    let mut synced_items = Vec::new();
    let mut new_albums_count = 0;
    let mut new_tracks_count = 0;
    let now = Utc::now().to_rfc3339();

    for fetched in fetched_albums.into_iter().flatten() {
        let manifest = fetched.manifest;
        let summary = fetched.summary;
        let local_art_path = fetched.local_art_path;
        let mut album_has_new = false;

        for track in &manifest.tracks {
            let is_new = !existing_remote_track_ids.contains(&track.track_id);
            if is_new {
                new_tracks_count += 1;
                album_has_new = true;
            }

            let remote_track_path = format!(
                "remote://{}/{}/{}",
                target.id, summary.relative_folder, track.relative_file_name
            );

            synced_tracks.push(ScannedTrack {
                id: track.track_id.clone(),
                path: remote_track_path,
                file_name: track.file_name.clone(),
                album_art_path: local_art_path.clone(),
                audio: track.audio.clone(),
                raw_tags: track.raw_tags.clone(),
                mapped_fields: track.mapped_fields.clone(),
            });

            synced_items.push(RemoteSyncedItem {
                item_id: track.track_id.clone(),
                remote_target_id: target.id.clone(),
                item_type: "track".to_string(),
                remote_path: format!("{}/{}", summary.relative_folder, track.relative_file_name),
                remote_file_id: None,
                size_bytes: track.file_size_bytes,
                checksum: track.audio_checksum_sha256.clone(),
                sync_status: "synced".to_string(),
                last_synced_at: now.clone(),
            });
        }

        if album_has_new {
            new_albums_count += 1;
        }

        synced_items.push(RemoteSyncedItem {
            item_id: manifest.album_id.clone(),
            remote_target_id: target.id.clone(),
            item_type: "album".to_string(),
            remote_path: summary.relative_folder.clone(),
            remote_file_id: None,
            size_bytes: summary.total_size_bytes,
            checksum: None,
            sync_status: "synced".to_string(),
            last_synced_at: now.clone(),
        });
    }

    if let Some(ref tx) = progress_tx {
        let _ = tx.send(RemoteSyncProgressEvent {
            target_id: target.id.clone(),
            target_name: target.name.clone(),
            current_album: "Processing albums...".to_string(),
            completed_albums: total_albums,
            total_albums,
            status: "albums_fetched".to_string(),
        });
    }

    Ok(SyncRemoteResult {
        target_id: target.id.clone(),
        target_name: target.name.clone(),
        synced_tracks,
        synced_items,
        new_albums_count,
        new_tracks_count,
        master_index_updated_at: Some(master_index.updated_at),
    })
}

/// Discovers and synchronizes remote playlists from a target.
/// Reads aria-playlists-index.json and each playlist manifest.
pub async fn sync_remote_playlists(
    backend: Arc<dyn RemoteStorageBackend>,
    target: &RemoteTarget,
) -> Result<Vec<RemotePlaylistManifest>, RemoteStorageError> {
    let index_bytes = match backend.read_bytes("aria-playlists-index.json").await {
        Ok(bytes) => bytes,
        Err(RemoteStorageError::NotFound(_)) => {
            return Ok(Vec::new());
        }
        Err(err) => return Err(err),
    };

    let index_str = String::from_utf8_lossy(&index_bytes);
    let playlist_index = deserialize_master_playlist_index(&index_str)
        .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

    let mut manifests = Vec::new();
    for summary in &playlist_index.playlists {
        match backend.read_bytes(&summary.relative_path).await {
            Ok(bytes) => {
                let manifest_str = String::from_utf8_lossy(&bytes);
                match deserialize_playlist_manifest(&manifest_str) {
                    Ok(m) => manifests.push(m),
                    Err(e) => {
                        tracing::warn!(
                            "Failed to deserialize playlist manifest for '{}' from target '{}': {e}",
                            summary.name,
                            target.name
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to read playlist manifest '{}' from target '{}': {e}",
                    summary.relative_path,
                    target.name
                );
            }
        }
    }

    Ok(manifests)
}
