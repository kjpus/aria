use aria_domain::{RemoteSyncedItem, RemoteTarget};
use aria_remote_storage::StorageStatus;
use tauri::State;

use crate::{error::CommandError, AppState};

#[tauri::command]
pub async fn get_remote_targets(
    state: State<'_, AppState>,
) -> Result<Vec<RemoteTarget>, CommandError> {
    Ok(state.core.get_remote_targets()?)
}

#[tauri::command]
pub async fn save_remote_target(
    state: State<'_, AppState>,
    target: RemoteTarget,
) -> Result<Vec<RemoteTarget>, CommandError> {
    Ok(state.core.save_remote_target(target)?)
}

#[tauri::command]
pub async fn delete_remote_target(
    state: State<'_, AppState>,
    target_id: String,
) -> Result<Vec<RemoteTarget>, CommandError> {
    Ok(state.core.delete_remote_target(&target_id)?)
}

#[tauri::command]
pub async fn test_remote_target(
    state: State<'_, AppState>,
    target_id: String,
) -> Result<StorageStatus, CommandError> {
    Ok(state.core.test_remote_target(&target_id).await?)
}

#[tauri::command]
pub async fn start_gdrive_auth_flow(
    state: State<'_, AppState>,
    client_id: String,
    client_secret: Option<String>,
) -> Result<String, CommandError> {
    Ok(state.core.start_gdrive_auth_flow(client_id, client_secret).await?)
}

#[tauri::command]
pub async fn complete_gdrive_auth_flow(
    state: State<'_, AppState>,
    target_name: String,
    storage_limit_bytes: Option<u64>,
) -> Result<RemoteTarget, CommandError> {
    Ok(state.core.complete_gdrive_auth_flow(target_name, storage_limit_bytes).await?)
}

#[tauri::command]
pub async fn upload_album_to_target(
    state: State<'_, AppState>,
    album_title: String,
    target_id: String,
) -> Result<(), CommandError> {
    Ok(state.core.upload_album_to_target(album_title, target_id).await?)
}

#[tauri::command]
pub async fn delete_album_from_target(
    state: State<'_, AppState>,
    album_id: String,
    target_id: String,
) -> Result<(), CommandError> {
    Ok(state.core.delete_album_from_target(&album_id, &target_id).await?)
}

#[tauri::command]
pub async fn get_remote_synced_items(
    state: State<'_, AppState>,
    target_id: String,
) -> Result<Vec<RemoteSyncedItem>, CommandError> {
    Ok(state.core.get_remote_synced_items(&target_id)?)
}
