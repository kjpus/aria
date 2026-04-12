use aria_domain::{PlayTrackRequest, PlaybackSnapshot};
use tauri::State;

use crate::{error::CommandError, AppState};

#[tauri::command]
pub async fn play(state: State<'_, AppState>) -> Result<PlaybackSnapshot, CommandError> {
    Ok(state.core.play().await?)
}

#[tauri::command]
pub async fn play_track(
    state: State<'_, AppState>,
    request: PlayTrackRequest,
) -> Result<PlaybackSnapshot, CommandError> {
    Ok(state.core.play_track(request).await?)
}

#[tauri::command]
pub async fn add_to_queue(
    state: State<'_, AppState>,
    requests: Vec<PlayTrackRequest>,
) -> Result<PlaybackSnapshot, CommandError> {
    Ok(state.core.add_to_queue(requests).await)
}

#[tauri::command]
pub async fn replace_queue(
    state: State<'_, AppState>,
    requests: Vec<PlayTrackRequest>,
    start_playing: bool,
) -> Result<PlaybackSnapshot, CommandError> {
    Ok(state.core.replace_queue(requests, start_playing).await?)
}

#[tauri::command]
pub async fn previous_track(state: State<'_, AppState>) -> Result<PlaybackSnapshot, CommandError> {
    Ok(state.core.previous_track().await?)
}

#[tauri::command]
pub async fn next_track(state: State<'_, AppState>) -> Result<PlaybackSnapshot, CommandError> {
    Ok(state.core.next_track().await?)
}

#[tauri::command]
pub async fn shuffle_queue(state: State<'_, AppState>) -> Result<PlaybackSnapshot, CommandError> {
    Ok(state.core.shuffle_queue().await)
}

#[tauri::command]
pub async fn restore_queue_order(
    state: State<'_, AppState>,
) -> Result<PlaybackSnapshot, CommandError> {
    Ok(state.core.restore_queue_order().await)
}

#[tauri::command]
pub async fn pause(state: State<'_, AppState>) -> Result<PlaybackSnapshot, CommandError> {
    Ok(state.core.pause().await)
}

#[tauri::command]
pub async fn seek(
    state: State<'_, AppState>,
    position_ms: u64,
) -> Result<PlaybackSnapshot, CommandError> {
    Ok(state.core.seek(position_ms).await?)
}
