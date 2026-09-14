use thiserror::Error;

#[derive(Debug, Error)]
pub enum RemoteStorageError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("Storage quota exceeded: required {required_bytes} bytes, but only {available_bytes} bytes available")]
    QuotaExceeded {
        required_bytes: u64,
        available_bytes: u64,
    },

    #[error("Authentication failed: {0}")]
    AuthFailed(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Remote provider error: {0}")]
    ProviderError(String),
}
