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
pub fn list_output_devices(
    state: State<'_, AppState>,
) -> Result<Vec<OutputDeviceSnapshot>, CommandError> {
    Ok(state.core.list_output_devices()?)
}

#[tauri::command]
pub async fn update_playback_preferences(
    state: State<'_, AppState>,
    playback: PlaybackPreferences,
) -> Result<SettingsSnapshot, CommandError> {
    Ok(state.core.update_playback_preferences(playback).await?)
}
