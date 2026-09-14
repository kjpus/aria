use std::sync::Arc;
use aria_domain::{AppEvent, CachePrefetchEvent, RemoteEvent, RemoteTarget};
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info};

use crate::cache::RemoteCacheManager;
use crate::factory::create_backend_for_target;

/// Descriptor of a remote track that can be prefetched.
#[derive(Debug, Clone)]
pub struct PrefetchCandidate {
    pub track_id: String,
    pub title: String,
    pub remote_path: String,
    pub expected_checksum: Option<String>,
    pub target: RemoteTarget,
}

/// Command sent to the prefetch background worker.
#[derive(Debug, Clone)]
pub enum PrefetchCommand {
    /// Request prefetching of tracks (typically N+1 and N+2 in queue).
    /// `active_window_track_ids`: list of track IDs representing the current sliding window [N, N+1, N+2].
    PrefetchTracks {
        candidates: Vec<PrefetchCandidate>,
        active_window_track_ids: Vec<String>,
    },
    /// Stop all prefetching.
    Cancel,
}

pub type PathUpdater = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Asynchronous background worker prefetching future tracks in the sliding window.
#[derive(Clone)]
pub struct PlaybackPrefetcher {
    command_tx: mpsc::Sender<PrefetchCommand>,
}

impl PlaybackPrefetcher {
    /// Spawns the prefetch worker loop on Tokio.
    pub fn spawn(
        cache_manager: Arc<RemoteCacheManager>,
        event_tx: Option<broadcast::Sender<AppEvent>>,
        path_updater: Option<PathUpdater>,
    ) -> Self {
        let (command_tx, mut command_rx) = mpsc::channel::<PrefetchCommand>(32);

        tokio::spawn(async move {
            while let Some(command) = command_rx.recv().await {
                match command {
                    PrefetchCommand::PrefetchTracks {
                        candidates,
                        active_window_track_ids,
                    } => {
                        for candidate in candidates {
                            // Skip if already in cache
                            if cache_manager.is_track_cached(&candidate.track_id).await {
                                if let Some(path) = cache_manager.get_cached_path(&candidate.track_id).await {
                                    if let Some(updater) = &path_updater {
                                        updater(&candidate.track_id, &path.to_string_lossy());
                                    }
                                }
                                continue;
                            }

                            info!(
                                "Prefetching remote track: '{}' ({})",
                                candidate.title, candidate.track_id
                            );

                            if let Some(events) = &event_tx {
                                let _ = events.send(AppEvent::Remote(RemoteEvent::CachePrefetchProgress(
                                    CachePrefetchEvent {
                                        track_id: candidate.track_id.clone(),
                                        title: candidate.title.clone(),
                                        bytes_downloaded: 0,
                                        total_bytes: 0,
                                        is_completed: false,
                                        error_message: None,
                                    },
                                )));
                            }

                            let backend = match create_backend_for_target(&candidate.target) {
                                Ok(backend) => backend,
                                Err(err) => {
                                    error!(
                                        "Failed to create backend for target '{}': {err}",
                                        candidate.target.id
                                    );
                                    if let Some(events) = &event_tx {
                                        let _ = events.send(AppEvent::Remote(RemoteEvent::CachePrefetchProgress(
                                            CachePrefetchEvent {
                                                track_id: candidate.track_id.clone(),
                                                title: candidate.title.clone(),
                                                bytes_downloaded: 0,
                                                total_bytes: 0,
                                                is_completed: true,
                                                error_message: Some(err.to_string()),
                                            },
                                        )));
                                    }
                                    continue;
                                }
                            };

                            match cache_manager
                                .get_or_download_track(
                                    backend.as_ref(),
                                    &candidate.remote_path,
                                    &candidate.track_id,
                                    candidate.expected_checksum.as_deref(),
                                    &active_window_track_ids,
                                )
                                .await
                            {
                                Ok(cached_path) => {
                                    info!(
                                        "Successfully prefetched track '{}' ({})",
                                        candidate.title, candidate.track_id
                                    );
                                    if let Some(updater) = &path_updater {
                                        updater(&candidate.track_id, &cached_path.to_string_lossy());
                                    }
                                    let size = tokio::fs::metadata(&cached_path)
                                        .await
                                        .map(|m| m.len())
                                        .unwrap_or(0);
                                    if let Some(events) = &event_tx {
                                        let _ = events.send(AppEvent::Remote(RemoteEvent::CachePrefetchProgress(
                                            CachePrefetchEvent {
                                                track_id: candidate.track_id.clone(),
                                                title: candidate.title.clone(),
                                                bytes_downloaded: size,
                                                total_bytes: size,
                                                is_completed: true,
                                                error_message: None,
                                            },
                                        )));
                                    }
                                }
                                Err(err) => {
                                    error!(
                                        "Failed to prefetch track '{}' ({}): {err}",
                                        candidate.title, candidate.track_id
                                    );
                                    if let Some(events) = &event_tx {
                                        let _ = events.send(AppEvent::Remote(RemoteEvent::CachePrefetchProgress(
                                            CachePrefetchEvent {
                                                track_id: candidate.track_id.clone(),
                                                title: candidate.title.clone(),
                                                bytes_downloaded: 0,
                                                total_bytes: 0,
                                                is_completed: true,
                                                error_message: Some(err.to_string()),
                                            },
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    PrefetchCommand::Cancel => {
                        // Clear pending commands in channel
                        while command_rx.try_recv().is_ok() {}
                    }
                }
            }
        });

        Self { command_tx }
    }

    /// Notifies the worker of tracks to prefetch in the background.
    pub async fn notify_prefetch(
        &self,
        candidates: Vec<PrefetchCandidate>,
        active_window_track_ids: Vec<String>,
    ) {
        let _ = self
            .command_tx
            .send(PrefetchCommand::PrefetchTracks {
                candidates,
                active_window_track_ids,
            })
            .await;
    }

    /// Cancels any scheduled prefetch downloads.
    pub async fn cancel(&self) {
        let _ = self.command_tx.send(PrefetchCommand::Cancel).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_domain::RemoteBackendType;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[tokio::test]
    async fn test_playback_prefetcher_worker() {
        let base_temp = std::env::temp_dir().join(format!("aria_prefetch_test_{}", std::process::id()));
        let storage_dir = base_temp.join("storage");
        let cache_dir = base_temp.join("cache");

        tokio::fs::create_dir_all(&storage_dir).await.unwrap();
        tokio::fs::create_dir_all(&cache_dir).await.unwrap();

        let track_file = storage_dir.join("track_1.flac");
        tokio::fs::write(&track_file, b"prefetch test audio content")
            .await
            .unwrap();

        let target = RemoteTarget {
            id: "fs-test-target".into(),
            name: "FS Test".into(),
            backend_type: RemoteBackendType::Filesystem,
            storage_limit_bytes: None,
            is_enabled: true,
            config_json: serde_json::json!({ "path": storage_dir.to_string_lossy() }).to_string(),
        };

        let cache_manager = Arc::new(RemoteCacheManager::new(cache_dir, 3));
        let updated = Arc::new(AtomicBool::new(false));
        let updated_clone = updated.clone();

        let path_updater: PathUpdater = Arc::new(move |_track_id, _path| {
            updated_clone.store(true, Ordering::SeqCst);
        });

        let prefetcher = PlaybackPrefetcher::spawn(cache_manager.clone(), None, Some(path_updater));

        let candidate = PrefetchCandidate {
            track_id: "track_1".into(),
            title: "Track 1".into(),
            remote_path: "track_1.flac".into(),
            expected_checksum: None,
            target,
        };

        prefetcher
            .notify_prefetch(vec![candidate], vec!["track_1".into()])
            .await;

        // Wait briefly for the worker to complete
        for _ in 0..50 {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            if cache_manager.is_track_cached("track_1").await {
                break;
            }
        }

        assert!(cache_manager.is_track_cached("track_1").await);
        assert!(updated.load(Ordering::SeqCst));

        let _ = tokio::fs::remove_dir_all(&base_temp).await;
    }
}
