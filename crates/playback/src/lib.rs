//! Long-lived audio playback engine, persistent queue management, `rubato`
//! resampling, Windows WASAPI exclusive output, and Android bit-perfect output.
//!
//! # Real-Time Audio Invariants
//! - Audio render threads never perform SQLite queries, artwork decoding, or
//!   unaligned PCM slice casts.
//! - `PlaybackService::shutdown` is idempotent and safe to invoke both inside
//!   and outside a Tokio runtime context.

pub(crate) mod resample;
mod service;

pub use service::{PlaybackError, PlaybackService};
#[cfg(target_os = "android")]
pub use service::register_android_context;











