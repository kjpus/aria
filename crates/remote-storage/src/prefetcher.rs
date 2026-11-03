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

        let worker = async move {
            let mut current_candidates: Vec<PrefetchCandidate> = Vec::new();
            let mut current_window_ids: Vec<String> = Vec::new();

            loop {
                // If there are no candidates left in the current window, wait for a new command
                if current_candidates.is_empty() {
                    match command_rx.recv().await {
                        Some(PrefetchCommand::PrefetchTracks {
                            candidates,
                            active_window_track_ids,
                        }) => {
                            aria_domain::log_diagnostic(
                                aria_domain::DiagnosticSubsystem::Download,
                                aria_domain::DiagnosticLogLevel::Debug,
                                format!(
                                    "Prefetcher: received {} candidates for active window {:?}",
                                    candidates.len(),
                                    active_window_track_ids
                                ),
                                None,
                            );
                            current_candidates = candidates;
                            current_window_ids = active_window_track_ids;
                        }
                        Some(PrefetchCommand::Cancel) => {
                            aria_domain::log_diagnostic(
                                aria_domain::DiagnosticSubsystem::Download,
                                aria_domain::DiagnosticLogLevel::Debug,
                                "Prefetcher: received Cancel command",
                                None,
                            );
                            current_candidates.clear();
                            current_window_ids.clear();
                        }
                        None => break,
                    }
                    continue;
                }

                let candidate = current_candidates.remove(0);

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
                    "Preemptively prefetching remote track: '{}' ({})",
                    candidate.title, candidate.track_id
                );
                aria_domain::log_diagnostic(
                    aria_domain::DiagnosticSubsystem::Download,
                    aria_domain::DiagnosticLogLevel::Info,
                    format!("Prefetcher: starting candidate '{}' (id: {})", candidate.title, candidate.track_id),
                    None,
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

                let window_ids = current_window_ids.clone();
                let candidate_clone = candidate.clone();
                let cache_mgr = cache_manager.clone();
                let download_fut = async move {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(90),
                        cache_mgr.get_or_download_track(
                            backend.as_ref(),
                            &candidate_clone.remote_path,
                            &candidate_clone.track_id,
                            candidate_clone.expected_checksum.as_deref(),
                            &window_ids,
                        ),
                    )
                    .await
                };

                tokio::pin!(download_fut);

                loop {
                    tokio::select! {
                        download_res = &mut download_fut => {
                            match download_res {
                                Ok(Ok(cached_path)) => {
                                    info!(
                                        "Successfully prefetched track '{}' ({})",
                                        candidate.title, candidate.track_id
                                    );
                                    aria_domain::log_diagnostic(
                                        aria_domain::DiagnosticSubsystem::Download,
                                        aria_domain::DiagnosticLogLevel::Info,
                                        format!("Prefetcher: successfully cached '{}' ({})", candidate.title, candidate.track_id),
                                        None,
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
                                Ok(Err(err)) => {
                                    error!(
                                        "Failed to prefetch track '{}' ({}): {err}",
                                        candidate.title, candidate.track_id
                                    );
                                    aria_domain::log_diagnostic(
                                        aria_domain::DiagnosticSubsystem::Download,
                                        aria_domain::DiagnosticLogLevel::Warn,
                                        format!("Prefetcher: failed candidate '{}' ({}): {err}", candidate.title, candidate.track_id),
                                        None,
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
                                Err(_) => {
                                    error!(
                                        "Prefetch download timed out for track '{}' ({})",
                                        candidate.title, candidate.track_id
                                    );
                                    aria_domain::log_diagnostic(
                                        aria_domain::DiagnosticSubsystem::Download,
                                        aria_domain::DiagnosticLogLevel::Warn,
                                        format!("Prefetcher: timed out (90s) for candidate '{}' ({})", candidate.title, candidate.track_id),
                                        None,
                                    );
                                    if let Some(events) = &event_tx {
                                        let _ = events.send(AppEvent::Remote(RemoteEvent::CachePrefetchProgress(
                                            CachePrefetchEvent {
                                                track_id: candidate.track_id.clone(),
                                                title: candidate.title.clone(),
                                                bytes_downloaded: 0,
                                                total_bytes: 0,
                                                is_completed: true,
                                                error_message: Some("Download timed out".into()),
                                            },
                                        )));
                                    }
                                }
                            }
                            break;
                        }
                        cmd = command_rx.recv() => {
                            match cmd {
                                Some(PrefetchCommand::PrefetchTracks {
                                    candidates: mut new_candidates,
                                    active_window_track_ids: new_window_ids,
                                }) => {
                                    new_candidates.retain(|c| c.track_id != candidate.track_id);
                                    current_candidates = new_candidates;
                                    current_window_ids = new_window_ids;

                                    if !current_window_ids.contains(&candidate.track_id) {
                                        info!(
                                            "Prefetch for track '{}' cancelled (no longer in active window)",
                                            candidate.title
                                        );
                                        break;
                                    }
                                    info!(
                                        "Prefetch for track '{}' kept in-flight despite sliding window shift",
                                        candidate.title
                                    );
                                }
                                Some(PrefetchCommand::Cancel) => {
                                    info!("Prefetch worker cancelled");
                                    current_candidates.clear();
                                    current_window_ids.clear();
                                    break;
                                }
                                None => return,
                            }
                        }
                    }
                }
            }
        };

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(worker);
        } else {
            std::thread::Builder::new()
                .name("aria-prefetcher".into())
                .stack_size(4 * 1024 * 1024)
                .spawn(move || {
                    let runtime = tokio::runtime::Builder::new_multi_thread()
                        .thread_stack_size(4 * 1024 * 1024)
                        .enable_all()
                        .build()
                        .expect("failed to create aria-prefetcher runtime");
                    runtime.block_on(worker);
                })
                .expect("failed to spawn aria-prefetcher thread");
        }

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
