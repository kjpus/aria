//! Local library scanning, raw-tag extraction, per-format database field
//! mapping, composer-aware catalog parsing, in-place tag editing, artwork
//! caching, and portable remote manifest builders.

mod error;
pub mod manifest;
mod service;

pub use error::LibraryError;
pub use service::{album_art_cache_root, LibraryService};
