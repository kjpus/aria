use std::path::{Path, PathBuf};
use async_trait::async_trait;
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use walkdir::WalkDir;

use crate::error::RemoteStorageError;
use crate::traits::RemoteStorageBackend;
use crate::types::{RemoteFileRef, StorageStatus, StorageUsage, TransferProgress};

#[derive(Debug, Clone)]
pub struct FilesystemBackend {
    root_path: PathBuf,
}

impl FilesystemBackend {
    pub fn new(root_path: impl Into<PathBuf>) -> Self {
        Self {
            root_path: root_path.into(),
        }
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    fn resolve_remote_path(&self, relative_path: &str) -> PathBuf {
        let normalized = relative_path.trim_start_matches(['/', '\\']);
        self.root_path.join(normalized)
    }
}

#[async_trait]
impl RemoteStorageBackend for FilesystemBackend {
    fn provider_id(&self) -> &'static str {
        "filesystem"
    }

    async fn test_connection(&self) -> Result<StorageStatus, RemoteStorageError> {
        let root = &self.root_path;
        if !root.exists() {
            if let Err(e) = fs::create_dir_all(root).await {
                return Ok(StorageStatus {
                    is_connected: false,
                    message: Some(format!("Cannot create root directory: {e}")),
                    storage_usage: None,
                });
            }
        }

        // Test write permission with a temp test file
        let test_file = root.join(".aria_storage_test");
        match fs::write(&test_file, b"aria_probe").await {
            Ok(_) => {
                let _ = fs::remove_file(&test_file).await;
                let usage = self.get_storage_usage().await.ok();
                Ok(StorageStatus {
                    is_connected: true,
                    message: Some("Directory accessible and writable".into()),
                    storage_usage: usage,
                })
            }
            Err(e) => Ok(StorageStatus {
                is_connected: false,
                message: Some(format!("Directory not writable: {e}")),
                storage_usage: None,
            }),
        }
    }

    async fn get_storage_usage(&self) -> Result<StorageUsage, RemoteStorageError> {
        let root = self.root_path.clone();

        // Calculate used bytes by traversing the root folder
        let used_bytes = tokio::task::spawn_blocking(move || {
            let mut total = 0u64;
            if root.exists() {
                for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
                    if let Ok(meta) = entry.metadata() {
                        if meta.is_file() {
                            total += meta.len();
                        }
                    }
                }
            }
            total
        })
        .await
        .map_err(|e| RemoteStorageError::ProviderError(format!("Task join error: {e}")))?;

        Ok(StorageUsage {
            used_bytes,
            total_bytes: None,
            free_bytes: None,
        })
    }

    async fn ensure_directory(&self, path: &str) -> Result<String, RemoteStorageError> {
        let target = self.resolve_remote_path(path);
        fs::create_dir_all(&target)
            .await
            .map_err(RemoteStorageError::Io)?;
        Ok(target.to_string_lossy().into_owned())
    }

    async fn upload_file(
        &self,
        local_path: &Path,
        remote_path: &str,
        progress: Option<mpsc::Sender<TransferProgress>>,
    ) -> Result<RemoteFileRef, RemoteStorageError> {
        let target_path = self.resolve_remote_path(remote_path);

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(RemoteStorageError::Io)?;
        }

        let mut source_file = fs::File::open(local_path)
            .await
            .map_err(RemoteStorageError::Io)?;
        let total_bytes = source_file
            .metadata()
            .await
            .map_err(RemoteStorageError::Io)?
            .len();

        // Write atomically using a temporary file
        let temp_path = target_path.with_extension(format!("tmp_{}", std::process::id()));
        let mut temp_file = fs::File::create(&temp_path)
            .await
            .map_err(RemoteStorageError::Io)?;

        let mut buffer = [0u8; 65536];
        let mut transferred = 0u64;

        loop {
            let bytes_read = source_file
                .read(&mut buffer)
                .await
                .map_err(RemoteStorageError::Io)?;
            if bytes_read == 0 {
                break;
            }

            temp_file
                .write_all(&buffer[..bytes_read])
                .await
                .map_err(RemoteStorageError::Io)?;
            transferred += bytes_read as u64;

            if let Some(ref p) = progress {
                let _ = p
                    .send(TransferProgress {
                        bytes_transferred: transferred,
                        total_bytes,
                    })
                    .await;
            }
        }

        temp_file.flush().await.map_err(RemoteStorageError::Io)?;
        drop(temp_file);

        // Atomic rename to final destination
        fs::rename(&temp_path, &target_path)
            .await
            .map_err(RemoteStorageError::Io)?;

        Ok(RemoteFileRef {
            remote_id: remote_path.to_string(),
            remote_path: target_path.to_string_lossy().into_owned(),
            size_bytes: total_bytes,
        })
    }

    async fn write_bytes(&self, remote_path: &str, data: &[u8]) -> Result<(), RemoteStorageError> {
        let target_path = self.resolve_remote_path(remote_path);

        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(RemoteStorageError::Io)?;
        }

        let temp_path = target_path.with_extension(format!("tmp_{}", std::process::id()));
        fs::write(&temp_path, data)
            .await
            .map_err(RemoteStorageError::Io)?;

        fs::rename(&temp_path, &target_path)
            .await
            .map_err(RemoteStorageError::Io)?;

        Ok(())
    }

    async fn read_bytes(&self, remote_path: &str) -> Result<Vec<u8>, RemoteStorageError> {
        let target_path = self.resolve_remote_path(remote_path);
        if !target_path.exists() {
            return Err(RemoteStorageError::NotFound(remote_path.to_string()));
        }
        fs::read(&target_path)
            .await
            .map_err(RemoteStorageError::Io)
    }

    async fn delete_file(&self, remote_path: &str) -> Result<(), RemoteStorageError> {
        let target_path = self.resolve_remote_path(remote_path);
        if target_path.exists() {
            fs::remove_file(&target_path)
                .await
                .map_err(RemoteStorageError::Io)?;
        }
        Ok(())
    }

    async fn delete_directory(&self, remote_path: &str) -> Result<(), RemoteStorageError> {
        let target_path = self.resolve_remote_path(remote_path);
        if target_path.exists() {
            fs::remove_dir_all(&target_path)
                .await
                .map_err(RemoteStorageError::Io)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_filesystem_backend_operations() {
        let temp_dir = std::env::temp_dir().join(format!("aria_fs_test_{}", std::process::id()));
        let backend = FilesystemBackend::new(&temp_dir);

        // 1. Connection test
        let status = backend.test_connection().await.expect("connection test");
        assert!(status.is_connected);

        // 2. Ensure directory
        let dir_path = backend
            .ensure_directory("Beethoven/Sym9")
            .await
            .expect("ensure directory");
        assert!(Path::new(&dir_path).exists());

        // 3. Write bytes (manifest)
        let manifest_content = b"{\"album\": \"Symphony 9\"}";
        backend
            .write_bytes("Beethoven/Sym9/aria-manifest.json", manifest_content)
            .await
            .expect("write manifest bytes");

        // 4. Read bytes
        let read_back = backend
            .read_bytes("Beethoven/Sym9/aria-manifest.json")
            .await
            .expect("read manifest");
        assert_eq!(read_back, manifest_content);

        // 5. Upload file
        let local_dummy = temp_dir.join("local_sample.flac");
        tokio::fs::write(&local_dummy, b"dummy audio content 12345")
            .await
            .expect("write local dummy");

        let uploaded_ref = backend
            .upload_file(
                &local_dummy,
                "Beethoven/Sym9/01 - Allegro.flac",
                None,
            )
            .await
            .expect("upload file");

        assert_eq!(uploaded_ref.size_bytes, 25);
        assert!(Path::new(&uploaded_ref.remote_path).exists());

        // 6. Check storage usage
        let usage = backend.get_storage_usage().await.expect("get usage");
        assert!(usage.used_bytes >= 25 + manifest_content.len() as u64);

        // 7. Delete file & directory
        backend
            .delete_file("Beethoven/Sym9/01 - Allegro.flac")
            .await
            .expect("delete file");
        assert!(!Path::new(&uploaded_ref.remote_path).exists());

        backend
            .delete_directory("Beethoven")
            .await
            .expect("delete dir");
        assert!(!backend.resolve_remote_path("Beethoven").exists());

        // Cleanup
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
