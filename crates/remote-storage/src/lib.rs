pub mod backends;
pub mod cache;
pub mod error;
pub mod factory;
pub mod pipeline;
pub mod prefetcher;
pub mod quota;
pub mod traits;
pub mod types;

pub use cache::{RemoteCacheManager, RemoteCacheStatus};
pub use prefetcher::{PlaybackPrefetcher, PrefetchCandidate, PrefetchCommand, PathUpdater};

pub use factory::{create_backend_for_target, FilesystemConfig, GoogleDriveStoredConfig};
pub use pipeline::{upload_album, UploadOptions};

pub use backends::{
    generate_pkce_challenge, generate_pkce_verifier, refresh_access_token, FilesystemBackend,
    GoogleAuthConfig, GoogleDriveBackend, GoogleTokens, PendingAuthFlow, SmbBackend, SmbConfig,
    WebDavBackend, WebDavConfig, DEFAULT_GOOGLE_CLIENT_ID,
};
pub use error::RemoteStorageError;
pub use quota::{ensure_within_quota, evaluate_quota, QuotaCheckResult};
pub use traits::RemoteStorageBackend;
pub use types::{RemoteFileRef, StorageStatus, StorageUsage, TransferProgress};
