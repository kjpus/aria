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
    #[error("select at least one track to export")]
    EmptyFieldExportSelection,
    #[error("select at least one track to edit")]
    EmptyTrackTagEditSelection,
    #[error("the selected field could not be exported")]
    InvalidFieldExportField,
    #[error("tag names cannot be empty")]
    InvalidFieldExportTag,
    #[error("one or more tag edits are invalid")]
    InvalidTrackTagEdit,
    #[error("the selected track is no longer in the library: {path}")]
    FieldExportTrackNotFound { path: String },
    #[error("could not write tags for {path}: {message}")]
    FieldExportWriteFailure { path: String, message: String },
    #[error("could not refresh track metadata after writing tags")]
    FieldExportRefreshFailure,
    #[error("could not refresh tags for {path} after writing")]
    FieldExportTrackRefreshFailure { path: String },
    #[error("invalid catalog rule for {label}: {message}")]
    InvalidCatalogRule { label: String, message: String },
}
