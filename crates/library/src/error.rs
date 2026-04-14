use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("library root cannot be empty")]
    EmptyRoot,
    #[error("library root already exists")]
    DuplicateRoot,
    #[error("a scan is already running")]
    AlreadyScanning,
    #[error("a track could not be parsed during scanning")]
    ScanFailure,
    #[error("the requested track could not be read from disk")]
    TrackReadFailure,
    #[error("invalid catalog rule for {label}: {message}")]
    InvalidCatalogRule { label: String, message: String },
}
