pub mod backends;
pub mod error;
pub mod quota;
pub mod traits;
pub mod types;

pub use backends::FilesystemBackend;
pub use error::RemoteStorageError;
pub use quota::{ensure_within_quota, evaluate_quota, QuotaCheckResult};
pub use traits::RemoteStorageBackend;
pub use types::{RemoteFileRef, StorageStatus, StorageUsage, TransferProgress};
