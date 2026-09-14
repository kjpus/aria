use std::path::Path;
use std::sync::Arc;

use aria_domain::{
    AlbumManifest, RemoteSyncedItem, RemoteTarget, ScannedTrack, UploadProgressEvent,
};
use aria_library::manifest::{
    build_album_manifest, build_master_album_summary, build_master_library_index,
    derive_systematic_folder_name, deserialize_master_index, serialize_manifest,
    serialize_master_index,
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

/// Executes the full upload pipeline for an album to a remote target.
pub async fn upload_album(
    backend: Arc<dyn RemoteStorageBackend>,
    target: &RemoteTarget,
    album_title: &str,
    tracks: &[ScannedTrack],
    options: UploadOptions,
    progress_tx: Option<broadcast::Sender<UploadProgressEvent>>,
) -> Result<Vec<RemoteSyncedItem>, RemoteStorageError> {
    if tracks.is_empty() {
        return Err(RemoteStorageError::ProviderError("No tracks in album".into()));
    }

    // 1. Calculate total size & pre-flight quota check
    let mut total_bytes = 0u64;
    for track in tracks {
        let p = Path::new(&track.path);
        let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        total_bytes += size;
    }

    // Include extra 100KB for manifest and index
    let total_upload_estimate = total_bytes + 100_000;
    ensure_within_quota(
        options.current_target_usage_bytes,
        total_upload_estimate,
        target.storage_limit_bytes,
    )?;

    // 2. Discover cover artwork
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

    // 3. Build album manifest
    let manifest: AlbumManifest = build_album_manifest(
        album_title,
        tracks,
        artwork_file_name.clone(),
        options.compute_checksums,
    )
    .map_err(RemoteStorageError::Io)?;

    let relative_folder = derive_systematic_folder_name(&manifest);
    let total_files = manifest.tracks.len() + if artwork_path.is_some() { 1 } else { 0 } + 1; // tracks + cover + manifest
    let mut completed_files = 0usize;
    let mut completed_bytes = 0u64;

    // 4. Ensure album folder exists
    backend.ensure_directory(&relative_folder).await?;

    // 5. Upload cover art if present
    if let Some(art_path) = artwork_path {
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
        let art_data = tokio::fs::read(&art_path)
            .await
            .map_err(RemoteStorageError::Io)?;
        backend.write_bytes(&remote_cover_path, &art_data).await?;
        completed_files += 1;
    }

    // 6. Upload audio files
    let mut synced_items = Vec::new();
    let now = Utc::now().to_rfc3339();

    for (_idx, (track, manifest_track)) in tracks.iter().zip(manifest.tracks.iter()).enumerate() {
        let local_path = Path::new(&track.path);
        let remote_file_path = format!("{relative_folder}/{}", manifest_track.relative_file_name);
        let track_size = manifest_track.file_size_bytes;

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

        let file_ref = backend
            .upload_file(local_path, &remote_file_path, None)
            .await?;

        completed_bytes += track_size;
        completed_files += 1;

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

        synced_items.push(RemoteSyncedItem {
            item_id: manifest_track.track_id.clone(),
            remote_target_id: target.id.clone(),
            item_type: "track".to_string(),
            remote_path: remote_file_path,
            remote_file_id: Some(file_ref.remote_id),
            size_bytes: track_size,
            checksum: manifest_track.audio_checksum_sha256.clone(),
            sync_status: "synced".to_string(),
            last_synced_at: now.clone(),
        });
    }

    // 7. Write aria-manifest.json
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
    let manifest_json = serialize_manifest(&manifest)
        .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;
    let remote_manifest_path = format!("{relative_folder}/aria-manifest.json");
    backend
        .write_bytes(&remote_manifest_path, manifest_json.as_bytes())
        .await?;
    completed_files += 1;

    // 8. Update root master index (aria-library-index.json)
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

    let master_json = serialize_master_index(&master_index)
        .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;
    backend
        .write_bytes("aria-library-index.json", master_json.as_bytes())
        .await?;

    // 9. Create album synced item
    synced_items.push(RemoteSyncedItem {
        item_id: manifest.album_id.clone(),
        remote_target_id: target.id.clone(),
        item_type: "album".to_string(),
        remote_path: relative_folder,
        remote_file_id: None,
        size_bytes: total_bytes,
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
        "completed",
    );

    Ok(synced_items)
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
