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

    #[error("Checksum mismatch for {file}: expected {expected}, calculated {calculated}")]
    ChecksumMismatch {
        file: String,
        expected: String,
        calculated: String,
    },
}

impl RemoteStorageError {
    pub fn auth_failed(msg: impl Into<String>) -> Self {
        let mut s = msg.into();
        while let Some(stripped) = s.strip_prefix("Authentication failed: ") {
            s = stripped.to_string();
        }
        while let Some(stripped) = s.strip_prefix("authentication failed: ") {
            s = stripped.to_string();
        }
        Self::AuthFailed(s)
    }
}
