//! SQLite persistence (`AppDatabase`), schema migrations, and settings storage
//! (`SettingsStore`). All connections enable `PRAGMA foreign_keys = ON` and a
//! 5-second `busy_timeout`.

mod database;
mod settings_store;

pub use database::{AppDatabase, PersistedState, StorageError};
pub use settings_store::SettingsStore;
