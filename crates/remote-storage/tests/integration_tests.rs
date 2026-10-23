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

#[tokio::test]
async fn test_sync_remote_library_full_pipeline() {
    use std::collections::HashSet;
    use std::sync::Arc;
    use aria_domain::{RemoteBackendType, RemoteTarget};
    use aria_remote_storage::{sync_remote_library, upload_album, UploadOptions};

    let temp_root = std::env::temp_dir().join(format!("aria_sync_test_{}", std::process::id()));
    let artwork_cache_dir = temp_root.join("local_artwork");
    let dummy_audio_dir = temp_root.join("source_music");
    tokio::fs::create_dir_all(&dummy_audio_dir).await.expect("create audio dir");

    let backend = Arc::new(FilesystemBackend::new(&temp_root.join("remote_store")));

    let target = RemoteTarget {
        id: "target_test_1".to_string(),
        name: "Test Remote Store".to_string(),
        backend_type: RemoteBackendType::Filesystem,
        storage_limit_bytes: None,
        is_enabled: true,
        config_json: String::new(),
    };

    // 1. Prepare dummy audio and cover files
    let dummy_audio_file = dummy_audio_dir.join("track1.flac");
    tokio::fs::write(&dummy_audio_file, b"MOCK_FLAC_STREAM_DATA").await.expect("write audio");
    let dummy_cover_file = dummy_audio_dir.join("cover.jpg");
    tokio::fs::write(&dummy_cover_file, b"MOCK_COVER_JPEG").await.expect("write cover");

    let mut mapped = BTreeMap::new();
    mapped.insert("composer".to_string(), vec!["Ludwig van Beethoven".to_string()]);
    mapped.insert("album".to_string(), vec!["Symphony No. 5 in C minor, Op. 67".to_string()]);
    mapped.insert("title".to_string(), vec!["I. Allegro con brio".to_string()]);
    mapped.insert("track_number".to_string(), vec!["1".to_string()]);

    let track = ScannedTrack {
        id: "track_lvb_5_1".to_string(),
        path: dummy_audio_file.to_string_lossy().to_string(),
        file_name: "track1.flac".to_string(),
        album_art_path: Some(dummy_cover_file.to_string_lossy().to_string()),
        audio: AudioPropertiesSnapshot {
            format: "FLAC".to_string(),
            duration_ms: 450_000,
            sample_rate: Some(44100),
            bit_depth: Some(16),
            channels: Some(2),
        },
        raw_tags: BTreeMap::new(),
        mapped_fields: mapped,
    };

    // 2. Upload album from computer 1
    let upload_options = UploadOptions {
        compute_checksums: false,
        current_target_usage_bytes: 0,
    };
    let upload_res = upload_album(
        backend.clone(),
        &target,
        "Symphony No. 5 in C minor, Op. 67",
        &[track],
        upload_options,
        None,
    )
    .await
    .expect("upload album");
    assert!(!upload_res.is_empty());

    // 3. Now simulate computer 2 syncing the remote library
    let mut known_tracks = HashSet::new();
    let empty_album_times = std::collections::HashMap::new();
    let sync_res = sync_remote_library(
        backend.clone(),
        &target,
        &artwork_cache_dir,
        &known_tracks,
        None,
        &empty_album_times,
        None,
    )
    .await
    .expect("sync remote library");

    assert_eq!(sync_res.new_albums_count, 1);
    assert_eq!(sync_res.new_tracks_count, 1);
    assert_eq!(sync_res.synced_tracks.len(), 1);
    assert!(sync_res.master_index_updated_at.is_some());
    assert!(sync_res.synced_tracks[0].path.starts_with("remote://target_test_1/"));
    assert_eq!(sync_res.synced_tracks[0].mapped_fields["composer"][0], "Ludwig van Beethoven");

    // Verify cover art was cached locally
    assert!(sync_res.synced_tracks[0].album_art_path.is_some());
    let cached_art = std::path::Path::new(sync_res.synced_tracks[0].album_art_path.as_ref().unwrap());
    assert!(cached_art.exists(), "cached artwork should exist on disk");

    // 4. Test syncing again when tracks are already known: should return 0 new
    for t in &sync_res.synced_tracks {
        known_tracks.insert(t.id.clone());
    }
    let mut album_times = std::collections::HashMap::new();
    if let Some(album_item) = sync_res.synced_items.iter().find(|i| i.item_type == "album") {
        album_times.insert(album_item.item_id.clone(), album_item.last_synced_at.clone());
    }
    let sync_res_again = sync_remote_library(
        backend.clone(),
        &target,
        &artwork_cache_dir,
        &known_tracks,
        None,
        &album_times,
        None,
    )
    .await
    .expect("sync again");
    assert_eq!(sync_res_again.new_albums_count, 0);
    assert_eq!(sync_res_again.new_tracks_count, 0);

    // 5. Test master index early-exit when target_last_synced_at matches
    let sync_res_early_exit = sync_remote_library(
        backend.clone(),
        &target,
        &artwork_cache_dir,
        &known_tracks,
        sync_res.master_index_updated_at.as_deref(),
        &album_times,
        None,
    )
    .await
    .expect("sync early exit");
    assert_eq!(sync_res_early_exit.new_albums_count, 0);
    assert_eq!(sync_res_early_exit.new_tracks_count, 0);
    assert!(sync_res_early_exit.synced_tracks.is_empty());

    // 6. Test uploading a playlist to the remote target
    use aria_domain::{Playlist, RemotePlaylistTrack};
    use aria_remote_storage::{sync_remote_playlists, upload_playlist};

    let playlist = Playlist {
        id: "playlist_fav_beethoven".to_string(),
        name: "Beethoven Masterpieces".to_string(),
        collage_seed: 7,
        track_ids: vec![sync_res.synced_tracks[0].id.clone()],
        created_at: Some("2026-10-22T10:00:00Z".to_string()),
    };

    let playlist_tracks = vec![RemotePlaylistTrack {
        track_id: sync_res.synced_tracks[0].id.clone(),
        relative_path: "Ludwig van Beethoven/Symphony No. 5 in C minor, Op. 67/01 - I. Allegro con brio.flac".to_string(),
        title: "I. Allegro con brio".to_string(),
        composer: Some("Ludwig van Beethoven".to_string()),
        album: Some("Symphony No. 5 in C minor, Op. 67".to_string()),
        duration_ms: 450_000,
    }];

    let playlist_item = upload_playlist(
        backend.clone(),
        &target,
        &playlist,
        &playlist_tracks,
        None,
    )
    .await
    .expect("upload playlist");

    assert_eq!(playlist_item.item_id, "playlist_fav_beethoven");
    assert_eq!(playlist_item.item_type, "playlist");
    assert_eq!(playlist_item.remote_path, "playlists/playlist_fav_beethoven.json");

    // Verify playlist files on remote storage
    let playlist_json_bytes = backend
        .read_bytes("playlists/playlist_fav_beethoven.json")
        .await
        .expect("read remote playlist json");
    assert!(!playlist_json_bytes.is_empty());

    let m3u8_bytes = backend
        .read_bytes("playlists/Beethoven Masterpieces.m3u8")
        .await
        .expect("read remote playlist m3u8");
    let m3u8_str = String::from_utf8_lossy(&m3u8_bytes);
    assert!(m3u8_str.starts_with("#EXTM3U"));
    assert!(m3u8_str.contains("../Ludwig van Beethoven/Symphony No. 5 in C minor, Op. 67/01 - I. Allegro con brio.flac"));

    let playlists_index_bytes = backend
        .read_bytes("aria-playlists-index.json")
        .await
        .expect("read remote playlists index");
    assert!(!playlists_index_bytes.is_empty());

    // 7. Test sync_remote_playlists discovers and deserializes the remote playlist
    let remote_playlists = sync_remote_playlists(backend.clone(), &target)
        .await
        .expect("sync remote playlists");
    assert_eq!(remote_playlists.len(), 1);
    assert_eq!(remote_playlists[0].playlist_id, "playlist_fav_beethoven");
    assert_eq!(remote_playlists[0].name, "Beethoven Masterpieces");
    assert_eq!(remote_playlists[0].tracks.len(), 1);
    assert_eq!(remote_playlists[0].tracks[0].track_id, sync_res.synced_tracks[0].id);

    // 8. Cleanup
    let _ = tokio::fs::remove_dir_all(&temp_root).await;
}

#[tokio::test]
async fn test_deduplication_and_skip_unchanged_tracks_and_playlists() {
    use std::sync::Arc;
    use aria_domain::{Playlist, RemoteBackendType, RemotePlaylistTrack, RemoteTarget};
    use aria_library::manifest::deserialize_manifest;
    use aria_remote_storage::{upload_album, upload_playlist, UploadOptions};

    let temp_root = std::env::temp_dir().join(format!("aria_dedup_test_{}", std::process::id()));
    let dummy_audio_dir = temp_root.join("source_music");
    tokio::fs::create_dir_all(&dummy_audio_dir).await.expect("create audio dir");

    let backend = Arc::new(FilesystemBackend::new(&temp_root.join("remote_store")));

    let target = RemoteTarget {
        id: "target_dedup_1".to_string(),
        name: "Dedup Remote Store".to_string(),
        backend_type: RemoteBackendType::Filesystem,
        storage_limit_bytes: Some(1_000_000),
        is_enabled: true,
        config_json: String::new(),
    };

    // 1. Prepare dummy audio tracks and cover
    let dummy_audio_file1 = dummy_audio_dir.join("track1.flac");
    tokio::fs::write(&dummy_audio_file1, b"AUDIO_DATA_FOR_TRACK_1").await.expect("write track 1");
    let dummy_audio_file2 = dummy_audio_dir.join("track2.flac");
    tokio::fs::write(&dummy_audio_file2, b"AUDIO_DATA_FOR_TRACK_2").await.expect("write track 2");
    let dummy_cover_file = dummy_audio_dir.join("cover.jpg");
    tokio::fs::write(&dummy_cover_file, b"COVER_ART_JPEG").await.expect("write cover");

    let mut mapped1 = BTreeMap::new();
    mapped1.insert("composer".to_string(), vec!["Johannes Brahms".to_string()]);
    mapped1.insert("album".to_string(), vec!["Piano Concerto No. 1 in D minor, Op. 15".to_string()]);
    mapped1.insert("title".to_string(), vec!["I. Maestoso".to_string()]);
    mapped1.insert("track_number".to_string(), vec!["1".to_string()]);

    let track1 = ScannedTrack {
        id: "track_brahms_1_1".to_string(),
        path: dummy_audio_file1.to_string_lossy().to_string(),
        file_name: "track1.flac".to_string(),
        album_art_path: Some(dummy_cover_file.to_string_lossy().to_string()),
        audio: AudioPropertiesSnapshot {
            format: "FLAC".to_string(),
            duration_ms: 600_000,
            sample_rate: Some(44100),
            bit_depth: Some(16),
            channels: Some(2),
        },
        raw_tags: BTreeMap::new(),
        mapped_fields: mapped1,
    };

    let mut mapped2 = BTreeMap::new();
    mapped2.insert("composer".to_string(), vec!["Johannes Brahms".to_string()]);
    mapped2.insert("album".to_string(), vec!["Piano Concerto No. 1 in D minor, Op. 15".to_string()]);
    mapped2.insert("title".to_string(), vec!["II. Adagio".to_string()]);
    mapped2.insert("track_number".to_string(), vec!["2".to_string()]);

    let track2 = ScannedTrack {
        id: "track_brahms_1_2".to_string(),
        path: dummy_audio_file2.to_string_lossy().to_string(),
        file_name: "track2.flac".to_string(),
        album_art_path: Some(dummy_cover_file.to_string_lossy().to_string()),
        audio: AudioPropertiesSnapshot {
            format: "FLAC".to_string(),
            duration_ms: 700_000,
            sample_rate: Some(44100),
            bit_depth: Some(16),
            channels: Some(2),
        },
        raw_tags: BTreeMap::new(),
        mapped_fields: mapped2,
    };

    // 2. Upload only Track 1 first (simulating a playlist upload containing only 1 track of an album)
    let upload_options = UploadOptions {
        compute_checksums: false,
        current_target_usage_bytes: 0,
    };
    let upload_res_1 = upload_album(
        backend.clone(),
        &target,
        "Piano Concerto No. 1 in D minor, Op. 15",
        &[track1.clone()],
        upload_options,
        None,
    )
    .await
    .expect("upload track 1");
    assert_eq!(upload_res_1.len(), 2); // 1 track + 1 album item

    // Verify remote manifest has 1 track
    let manifest_bytes = backend
        .read_bytes("Johannes Brahms/Piano Concerto No. 1 in D minor, Op. 15/aria-manifest.json")
        .await
        .expect("read manifest");
    let manifest: aria_domain::AlbumManifest =
        deserialize_manifest(&String::from_utf8_lossy(&manifest_bytes)).expect("deserialize manifest");
    assert_eq!(manifest.tracks.len(), 1);
    assert_eq!(manifest.tracks[0].relative_file_name, "01 - I. Maestoso.flac");

    // 3. Now upload Track 1 and Track 2 together
    // Track 1 should be recognized as already uploaded with identical content and skipped.
    // Track 2 should be uploaded.
    // The remote manifest should now contain BOTH tracks (merged).
    let upload_options_2 = UploadOptions {
        compute_checksums: false,
        current_target_usage_bytes: 50_000,
    };
    let upload_res_2 = upload_album(
        backend.clone(),
        &target,
        "Piano Concerto No. 1 in D minor, Op. 15",
        &[track1.clone(), track2.clone()],
        upload_options_2,
        None,
    )
    .await
    .expect("upload track 1 and 2");
    assert_eq!(upload_res_2.len(), 3); // 2 tracks + 1 album item

    let manifest_bytes_2 = backend
        .read_bytes("Johannes Brahms/Piano Concerto No. 1 in D minor, Op. 15/aria-manifest.json")
        .await
        .expect("read manifest 2");
    let manifest_2: aria_domain::AlbumManifest =
        deserialize_manifest(&String::from_utf8_lossy(&manifest_bytes_2)).expect("deserialize manifest 2");
    assert_eq!(manifest_2.tracks.len(), 2);
    assert_eq!(manifest_2.tracks[0].relative_file_name, "01 - I. Maestoso.flac");
    assert_eq!(manifest_2.tracks[1].relative_file_name, "02 - II. Adagio.flac");

    // 4. Test uploading again when ALL tracks are already uploaded and unchanged
    // Even if storage usage is near quota limit, pre-flight check should pass because 0 new bytes are uploaded
    let near_limit_options = UploadOptions {
        compute_checksums: false,
        current_target_usage_bytes: 999_900, // Near 1MB limit; new bytes would exceed it
    };
    let upload_res_3 = upload_album(
        backend.clone(),
        &target,
        "Piano Concerto No. 1 in D minor, Op. 15",
        &[track1.clone(), track2.clone()],
        near_limit_options,
        None,
    )
    .await
    .expect("upload unchanged album near quota limit should succeed");
    assert_eq!(upload_res_3.len(), 3);

    // 5. Test playlist upload deduplication
    let playlist = Playlist {
        id: "playlist_brahms".to_string(),
        name: "Brahms Favorites".to_string(),
        collage_seed: 42,
        track_ids: vec![track1.id.clone(), track2.id.clone()],
        created_at: Some("2026-10-22T10:00:00Z".to_string()),
    };

    let playlist_tracks = vec![
        RemotePlaylistTrack {
            track_id: track1.id.clone(),
            relative_path: "Johannes Brahms/Piano Concerto No. 1 in D minor, Op. 15/01 - I. Maestoso.flac".to_string(),
            title: "I. Maestoso".to_string(),
            composer: Some("Johannes Brahms".to_string()),
            album: Some("Piano Concerto No. 1 in D minor, Op. 15".to_string()),
            duration_ms: 600_000,
        },
        RemotePlaylistTrack {
            track_id: track2.id.clone(),
            relative_path: "Johannes Brahms/Piano Concerto No. 1 in D minor, Op. 15/02 - II. Adagio.flac".to_string(),
            title: "II. Adagio".to_string(),
            composer: Some("Johannes Brahms".to_string()),
            album: Some("Piano Concerto No. 1 in D minor, Op. 15".to_string()),
            duration_ms: 700_000,
        },
    ];

    let p_item_1 = upload_playlist(
        backend.clone(),
        &target,
        &playlist,
        &playlist_tracks,
        None,
    )
    .await
    .expect("upload playlist 1st time");
    assert_eq!(p_item_1.item_id, "playlist_brahms");

    // Upload identical playlist second time: succeeds and skips rewriting
    let p_item_2 = upload_playlist(
        backend.clone(),
        &target,
        &playlist,
        &playlist_tracks,
        None,
    )
    .await
    .expect("upload playlist 2nd time");
    assert_eq!(p_item_2.item_id, "playlist_brahms");

    // Cleanup
    let _ = tokio::fs::remove_dir_all(&temp_root).await;
}
