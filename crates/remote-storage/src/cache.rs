use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use sha2::{Digest, Sha256};
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;

use crate::error::RemoteStorageError;
use crate::traits::RemoteStorageBackend;

/// Cache status metadata for UI and monitoring.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCacheStatus {
    pub cache_dir: String,
    pub total_cached_tracks: usize,
    pub total_cached_bytes: u64,
    pub window_size: usize,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    file_path: PathBuf,
    size_bytes: u64,
    last_accessed: Instant,
}

/// Thread-safe in-memory LRU remote audio cache manager.
/// Audio files are stored locally under `remote_cache/<track_id>.<ext>`.
/// Writes are atomic (.tmp_<pid> renamed only on complete write + SHA-256 verification).
#[derive(Clone)]
pub struct RemoteCacheManager {
    cache_dir: PathBuf,
    window_size: usize,
    entries: Arc<RwLock<HashMap<String, CacheEntry>>>,
}

impl RemoteCacheManager {
    pub fn new(cache_dir: PathBuf, window_size: usize) -> Self {
        Self {
            cache_dir,
            window_size: window_size.max(1),
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Resolves the default local cache directory:
    /// `%LOCALAPPDATA%/Aria/remote_cache` on Windows, or standard cache on Linux/macOS.
    pub fn default_cache_dir() -> Result<PathBuf, RemoteStorageError> {
        let base_dir = dirs::data_local_dir()
            .or_else(|| std::env::current_dir().ok().map(|dir| dir.join(".cache")))
            .ok_or_else(|| {
                RemoteStorageError::ProviderError("Could not resolve local app data directory".into())
            })?;
        Ok(base_dir.join("Aria").join("remote_cache"))
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    pub fn window_size(&self) -> usize {
        self.window_size
    }

    /// Checks if a track is completely cached on disk.
    pub async fn is_track_cached(&self, track_id: &str) -> bool {
        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(track_id) {
            entry.file_path.exists()
        } else {
            false
        }
    }

    /// Returns the cached file path for a track if available.
    pub async fn get_cached_path(&self, track_id: &str) -> Option<PathBuf> {
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(track_id) {
            if entry.file_path.exists() {
                entry.last_accessed = Instant::now();
                return Some(entry.file_path.clone());
            }
        }
        None
    }

    /// Ensures the track is downloaded from the backend into cache.
    /// If already present, returns the local path immediately.
    /// If not present:
    /// 1. Reads raw bytes from `backend.read_bytes(remote_path)`
    /// 2. Verifies SHA-256 checksum if provided
    /// 3. Writes atomically to `.tmp_<pid>` and renames to target file
    /// 4. Records into in-memory LRU tracking and triggers eviction of items outside active window
    pub async fn get_or_download_track(
        &self,
        backend: &dyn RemoteStorageBackend,
        remote_path: &str,
        track_id: &str,
        expected_checksum_sha256: Option<&str>,
        active_window_track_ids: &[String],
    ) -> Result<PathBuf, RemoteStorageError> {
        // 1. Fast check if already cached
        if let Some(path) = self.get_cached_path(track_id).await {
            return Ok(path);
        }

        // 2. Ensure cache dir exists
        fs::create_dir_all(&self.cache_dir)
            .await
            .map_err(RemoteStorageError::Io)?;

        // Determine target file extension from remote path (default: .flac)
        let ext = Path::new(remote_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("flac");

        let target_file_name = format!("{track_id}.{ext}");
        let target_path = self.cache_dir.join(&target_file_name);

        // Check if file is already on disk from previous session
        if target_path.exists() {
            let metadata = fs::metadata(&target_path)
                .await
                .map_err(RemoteStorageError::Io)?;
            let size = metadata.len();
            let mut entries = self.entries.write().await;
            entries.insert(
                track_id.to_string(),
                CacheEntry {
                    file_path: target_path.clone(),
                    size_bytes: size,
                    last_accessed: Instant::now(),
                },
            );
            return Ok(target_path);
        }

        // 3. Download from remote backend
        let bytes = backend.read_bytes(remote_path).await?;

        // 4. Validate checksum if present
        if let Some(expected) = expected_checksum_sha256 {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let calculated = format!("{:x}", hasher.finalize());
            if !calculated.eq_ignore_ascii_case(expected) {
                return Err(RemoteStorageError::ChecksumMismatch {
                    file: remote_path.to_string(),
                    expected: expected.to_string(),
                    calculated,
                });
            }
        }

        // 5. Atomic write: write to temp file, then rename
        let temp_file_name = format!("{target_file_name}.tmp_{}", std::process::id());
        let temp_path = self.cache_dir.join(temp_file_name);

        {
            let mut file = File::create(&temp_path)
                .await
                .map_err(RemoteStorageError::Io)?;
            file.write_all(&bytes).await.map_err(RemoteStorageError::Io)?;
            file.flush().await.map_err(RemoteStorageError::Io)?;
        }

        fs::rename(&temp_path, &target_path)
            .await
            .map_err(RemoteStorageError::Io)?;

        let size_bytes = bytes.len() as u64;

        // 6. Record in LRU entries
        {
            let mut entries = self.entries.write().await;
            entries.insert(
                track_id.to_string(),
                CacheEntry {
                    file_path: target_path.clone(),
                    size_bytes,
                    last_accessed: Instant::now(),
                },
            );
        }

        // 7. Evict tracks outside the active window if cache exceeds window_size
        self.evict_outside_window(active_window_track_ids).await;

        Ok(target_path)
    }

    /// Evicts cached tracks that are NOT part of the currently active window
    /// in least-recently-accessed order until cache count <= window_size.
    pub async fn evict_outside_window(&self, active_window_track_ids: &[String]) {
        let active_set: std::collections::HashSet<&str> =
            active_window_track_ids.iter().map(|s| s.as_str()).collect();

        let mut entries = self.entries.write().await;
        if entries.len() <= self.window_size {
            return;
        }

        // Candidates for eviction: tracks NOT in the active window
        let mut candidates: Vec<(String, Instant, PathBuf)> = entries
            .iter()
            .filter(|(id, _)| !active_set.contains(id.as_str()))
            .map(|(id, entry)| (id.clone(), entry.last_accessed, entry.file_path.clone()))
            .collect();

        // Sort by last_accessed ascending (oldest first)
        candidates.sort_by_key(|(_, accessed, _)| *accessed);

        for (id, _, path) in candidates {
            if entries.len() <= self.window_size {
                break;
            }

            if path.exists() {
                let _ = std::fs::remove_file(&path);
            }
            entries.remove(&id);
        }
    }

    /// Returns cache statistics.
    pub async fn get_status(&self) -> RemoteCacheStatus {
        let entries = self.entries.read().await;
        let total_cached_tracks = entries.len();
        let total_cached_bytes = entries.values().map(|e| e.size_bytes).sum();

        RemoteCacheStatus {
            cache_dir: self.cache_dir.display().to_string(),
            total_cached_tracks,
            total_cached_bytes,
            window_size: self.window_size,
        }
    }

    /// Clears all files in the cache.
    pub async fn clear(&self) -> Result<(), RemoteStorageError> {
        let mut entries = self.entries.write().await;
        entries.clear();

        if self.cache_dir.exists() {
            let mut read_dir = fs::read_dir(&self.cache_dir)
                .await
                .map_err(RemoteStorageError::Io)?;
            while let Ok(Some(entry)) = read_dir.next_entry().await {
                let path = entry.path();
                if path.is_file() {
                    let _ = fs::remove_file(path).await;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::filesystem::FilesystemBackend;

    #[tokio::test]
    async fn test_cache_sliding_window_eviction() {
        let temp_dir = std::env::temp_dir().join(format!("aria_cache_test_{}", std::process::id()));
        let remote_dir = temp_dir.join("remote");
        let cache_dir = temp_dir.join("cache");

        std::fs::create_dir_all(&remote_dir).unwrap();

        // Set up mock remote backend with 5 audio files
        let backend = FilesystemBackend::new(&remote_dir);
        for i in 1..=5 {
            let file_name = format!("track_{i}.flac");
            backend
                .write_bytes(&file_name, format!("FLAC DATA FOR TRACK {i}").as_bytes())
                .await
                .unwrap();
        }

        // Cache manager with window size 3
        let cache_manager = RemoteCacheManager::new(cache_dir.clone(), 3);

        // Download track 1 (active window: [track_1])
        let p1 = cache_manager
            .get_or_download_track(&backend, "track_1.flac", "trk_1", None, &["trk_1".into()])
            .await
            .unwrap();
        assert!(p1.exists());

        // Download track 2 (active window: [trk_1, trk_2])
        let p2 = cache_manager
            .get_or_download_track(
                &backend,
                "track_2.flac",
                "trk_2",
                None,
                &["trk_1".into(), "trk_2".into()],
            )
            .await
            .unwrap();
        assert!(p2.exists());

        // Download track 3 (active window: [trk_1, trk_2, trk_3])
        let p3 = cache_manager
            .get_or_download_track(
                &backend,
                "track_3.flac",
                "trk_3",
                None,
                &["trk_1".into(), "trk_2".into(), "trk_3".into()],
            )
            .await
            .unwrap();
        assert!(p3.exists());

        let status = cache_manager.get_status().await;
        assert_eq!(status.total_cached_tracks, 3);

        // Window slides forward: active window is now [trk_2, trk_3, trk_4]
        // Track 4 is downloaded; Track 1 is outside the window and oldest, so it should be evicted
        let p4 = cache_manager
            .get_or_download_track(
                &backend,
                "track_4.flac",
                "trk_4",
                None,
                &["trk_2".into(), "trk_3".into(), "trk_4".into()],
            )
            .await
            .unwrap();
        assert!(p4.exists());

        let status_after_slide = cache_manager.get_status().await;
        assert_eq!(status_after_slide.total_cached_tracks, 3);

        // trk_1 should be evicted from disk and memory
        assert!(!p1.exists());
        assert!(!cache_manager.is_track_cached("trk_1").await);

        // trk_2, trk_3, trk_4 remain
        assert!(cache_manager.is_track_cached("trk_2").await);
        assert!(cache_manager.is_track_cached("trk_3").await);
        assert!(cache_manager.is_track_cached("trk_4").await);
    }

    #[tokio::test]
    async fn test_cache_checksum_mismatch_rejected() {
        let temp_dir = std::env::temp_dir().join(format!("aria_cache_test_bad_{}", std::process::id()));
        let remote_dir = temp_dir.join("remote");
        let cache_dir = temp_dir.join("cache");

        std::fs::create_dir_all(&remote_dir).unwrap();

        let backend = FilesystemBackend::new(&remote_dir);
        backend
            .write_bytes("corrupted.flac", b"REAL DATA")
            .await
            .unwrap();

        let cache_manager = RemoteCacheManager::new(cache_dir, 3);

        // Expect incorrect checksum
        let bad_checksum = "0000000000000000000000000000000000000000000000000000000000000000";
        let res = cache_manager
            .get_or_download_track(
                &backend,
                "corrupted.flac",
                "trk_bad",
                Some(bad_checksum),
                &[],
            )
            .await;

        assert!(res.is_err());
        assert!(!cache_manager.is_track_cached("trk_bad").await);
    }
}
