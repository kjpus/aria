use std::collections::BTreeMap;

use aria_domain::{AudioPropertiesSnapshot, ScannedTrack};
use aria_library::manifest::{
    build_album_manifest, build_master_album_summary, build_master_library_index,
    deserialize_manifest, deserialize_master_index, derive_systematic_folder_name,
    serialize_manifest, serialize_master_index,
};
use aria_remote_storage::{ensure_within_quota, evaluate_quota, FilesystemBackend, RemoteStorageBackend};

#[tokio::test]
async fn test_full_remote_album_upload_and_master_index_lifecycle() {
    let temp_root = std::env::temp_dir().join(format!("aria_int_test_{}", std::process::id()));
    let backend = FilesystemBackend::new(&temp_root);

    // 1. Prepare sample tracks
    let mut mapped1 = BTreeMap::new();
    mapped1.insert("composer".to_string(), vec!["Johannes Brahms".to_string()]);
    mapped1.insert("album".to_string(), vec!["Symphony No. 1 in C minor, Op. 68".to_string()]);
    mapped1.insert("title".to_string(), vec!["I. Un poco sostenuto - Allegro".to_string()]);
    mapped1.insert("track_number".to_string(), vec!["1".to_string()]);
    mapped1.insert("catalog".to_string(), vec!["Op. 68".to_string()]);

    let track1 = ScannedTrack {
        id: "local_t1".to_string(),
        path: "dummy_brahms_1.flac".to_string(),
        file_name: "brahms_1.flac".to_string(),
        album_art_path: None,
        audio: AudioPropertiesSnapshot {
            format: "FLAC".to_string(),
            duration_ms: 840_000,
            sample_rate: Some(96000),
            bit_depth: Some(24),
            channels: Some(2),
        },
        raw_tags: BTreeMap::new(),
        mapped_fields: mapped1,
    };

    // 2. Build Album Manifest
    let manifest = build_album_manifest(
        "Symphony No. 1 in C minor, Op. 68",
        &[track1],
        Some("cover.jpg".to_string()),
        false,
    )
    .expect("build manifest");

    let relative_folder = derive_systematic_folder_name(&manifest);
    assert_eq!(relative_folder, "Johannes Brahms/Symphony No. 1 in C minor, Op. 68");

    // 3. Quota pre-flight check
    let estimated_bytes = manifest.tracks.iter().map(|t| t.file_size_bytes).sum::<u64>() + 50_000;
    let quota_limit = Some(500_000_000); // 500 MB
    let quota_check = evaluate_quota(0, estimated_bytes, quota_limit);
    assert!(quota_check.fits);
    assert!(ensure_within_quota(0, estimated_bytes, quota_limit).is_ok());

    // 4. Ensure album directory exists on remote target
    backend
        .ensure_directory(&relative_folder)
        .await
        .expect("ensure album folder");

    // 5. Upload simulated audio file
    let dummy_audio_file = temp_root.join("local_source_brahms.flac");
    tokio::fs::write(&dummy_audio_file, b"MOCK_FLAC_AUDIO_PAYLOAD")
        .await
        .expect("write dummy audio");

    let remote_audio_path = format!("{}/{}", relative_folder, manifest.tracks[0].relative_file_name);
    let uploaded_ref = backend
        .upload_file(&dummy_audio_file, &remote_audio_path, None)
        .await
        .expect("upload track audio");
    assert_eq!(uploaded_ref.size_bytes, 23);

    // 6. Write aria-manifest.json into the album directory
    let manifest_json = serialize_manifest(&manifest).expect("serialize manifest");
    let remote_manifest_path = format!("{}/aria-manifest.json", relative_folder);
    backend
        .write_bytes(&remote_manifest_path, manifest_json.as_bytes())
        .await
        .expect("write manifest to remote");

    // 7. Write cover.jpg
    let remote_cover_path = format!("{}/cover.jpg", relative_folder);
    backend
        .write_bytes(&remote_cover_path, b"MOCK_JPEG_DATA")
        .await
        .expect("write cover to remote");

    // 8. Create and write root master index (aria-library-index.json)
    let album_summary = build_master_album_summary(&manifest, &relative_folder);
    let master_index = build_master_library_index(vec![album_summary]);
    let master_json = serialize_master_index(&master_index).expect("serialize master index");
    backend
        .write_bytes("aria-library-index.json", master_json.as_bytes())
        .await
        .expect("write master index");

    // 9. Verify client can read and deserialize both
    let read_index_bytes = backend
        .read_bytes("aria-library-index.json")
        .await
        .expect("read master index");
    let read_index = deserialize_master_index(std::str::from_utf8(&read_index_bytes).unwrap())
        .expect("deserialize master index");
    assert_eq!(read_index.albums.len(), 1);
    assert_eq!(read_index.albums[0].album_id, manifest.album_id);

    let read_manifest_bytes = backend
        .read_bytes(&remote_manifest_path)
        .await
        .expect("read album manifest");
    let read_manifest = deserialize_manifest(std::str::from_utf8(&read_manifest_bytes).unwrap())
        .expect("deserialize album manifest");
    assert_eq!(read_manifest.album_id, manifest.album_id);
    assert_eq!(read_manifest.tracks[0].track_id, manifest.tracks[0].track_id);

    // 10. Check storage usage
    let usage = backend.get_storage_usage().await.expect("storage usage");
    assert!(usage.used_bytes > 0);

    // 11. Cleanup test files
    let _ = tokio::fs::remove_dir_all(&temp_root).await;
}
