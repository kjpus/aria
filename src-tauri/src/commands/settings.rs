use aria_domain::{
    OutputDeviceSnapshot, PlaybackPreferences, SettingsSnapshot, ThemePreference,
    TrackTableSettings,
};
use tauri::State;

use crate::{error::CommandError, AppState};

#[tauri::command]
pub async fn update_theme(
    state: State<'_, AppState>,
    theme: ThemePreference,
) -> Result<SettingsSnapshot, CommandError> {
    Ok(state.core.update_theme(theme).await)
}

#[tauri::command]
pub async fn update_track_table_settings(
    state: State<'_, AppState>,
    track_table: TrackTableSettings,
) -> Result<SettingsSnapshot, CommandError> {
    Ok(state.core.update_track_table_settings(track_table).await)
}

#[tauri::command]
pub async fn update_album_track_table_settings(
    state: State<'_, AppState>,
    album_track_table: TrackTableSettings,
) -> Result<SettingsSnapshot, CommandError> {
    Ok(state
        .core
        .update_album_track_table_settings(album_track_table)
        .await)
}

#[tauri::command]
pub async fn update_playlist_track_table_settings(
    state: State<'_, AppState>,
    playlist_track_table: TrackTableSettings,
) -> Result<SettingsSnapshot, CommandError> {
    Ok(state
        .core
        .update_playlist_track_table_settings(playlist_track_table)
        .await)
}

#[tauri::command]
pub fn list_output_devices(
    state: State<'_, AppState>,
) -> Result<Vec<OutputDeviceSnapshot>, CommandError> {
    Ok(state.core.list_output_devices()?)
}

#[tauri::command]
pub async fn refresh_output_devices(
    state: State<'_, AppState>,
) -> Result<Vec<OutputDeviceSnapshot>, CommandError> {
    Ok(state.core.refresh_output_devices().await?)
}

#[tauri::command]
pub async fn update_playback_preferences(
    state: State<'_, AppState>,
    playback: PlaybackPreferences,
) -> Result<SettingsSnapshot, CommandError> {
    Ok(state.core.update_playback_preferences(playback).await?)
}

#[tauri::command]
pub async fn update_auto_sync_on_startup(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<SettingsSnapshot, CommandError> {
    Ok(state.core.update_auto_sync_on_startup(enabled).await)
}

#[tauri::command]
pub async fn update_remote_storage_only(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<SettingsSnapshot, CommandError> {
    Ok(state.core.update_remote_storage_only(enabled).await)
}

#[tauri::command]
pub async fn update_logging_settings(
    state: State<'_, AppState>,
    settings: aria_domain::LoggingSettings,
) -> Result<SettingsSnapshot, CommandError> {
    Ok(state.core.update_logging_settings(settings).await)
}

#[tauri::command]
pub fn get_diagnostic_logs(
    state: State<'_, AppState>,
    subsystem: Option<String>,
    min_level: Option<aria_domain::DiagnosticLogLevel>,
    limit: Option<usize>,
) -> Result<Vec<aria_domain::DiagnosticLogEntry>, CommandError> {
    let sub = subsystem.as_deref().map(aria_domain::DiagnosticSubsystem::from_str_loose);
    Ok(state.core.get_diagnostic_logs(sub, min_level, limit))
}

#[tauri::command]
pub fn clear_diagnostic_logs(state: State<'_, AppState>) -> Result<(), CommandError> {
    state.core.clear_diagnostic_logs();
    Ok(())
}

#[tauri::command]
pub fn export_diagnostic_logs_text(state: State<'_, AppState>) -> Result<String, CommandError> {
    Ok(state.core.export_diagnostic_logs_text())
}

#[tauri::command]
pub fn get_diagnostic_log_path(state: State<'_, AppState>) -> Result<Option<String>, CommandError> {
    Ok(state.core.get_diagnostic_log_path())
}
