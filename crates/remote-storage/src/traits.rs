use std::path::Path;
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::RemoteStorageError;
use crate::types::{RemoteFileRef, StorageStatus, StorageUsage, TransferProgress};

#[async_trait]
pub trait RemoteStorageBackend: Send + Sync {
    /// Identifier of the provider (e.g., "google_drive", "filesystem").
    fn provider_id(&self) -> &'static str;

    /// Tests the connectivity and authentication of the remote target.
    async fn test_connection(&self) -> Result<StorageStatus, RemoteStorageError>;

    /// Retrieves current storage usage (used, total, free) if supported.
    async fn get_storage_usage(&self) -> Result<StorageUsage, RemoteStorageError>;

    /// Ensures a remote directory exists, creating intermediate folders if necessary.
    /// Returns the remote directory identifier or resolved path.
    async fn ensure_directory(&self, path: &str) -> Result<String, RemoteStorageError>;

    /// Uploads a local file to the specified remote path.
    async fn upload_file(
        &self,
        local_path: &Path,
        remote_path: &str,
        progress: Option<mpsc::Sender<TransferProgress>>,
    ) -> Result<RemoteFileRef, RemoteStorageError>;

    /// Writes raw byte content directly to a remote path (e.g. for manifests or indexes).
    async fn write_bytes(&self, remote_path: &str, data: &[u8]) -> Result<(), RemoteStorageError>;

    /// Reads raw byte content from a remote path.
    async fn read_bytes(&self, remote_path: &str) -> Result<Vec<u8>, RemoteStorageError>;

    /// Deletes a single remote file.
    async fn delete_file(&self, remote_path: &str) -> Result<(), RemoteStorageError>;

    /// Deletes a remote directory and its contents recursively.
    async fn delete_directory(&self, remote_path: &str) -> Result<(), RemoteStorageError>;
}
