use aria_domain::{
    CatalogRule, FieldExportRequest, LibraryFieldMapping, LibrarySnapshot, TrackTagEditRequest,
};
use std::collections::BTreeMap;
use tauri::State;

use crate::{error::CommandError, AppState};

#[tauri::command]
pub async fn add_library_root(
    state: State<'_, AppState>,
    path: String,
) -> Result<LibrarySnapshot, CommandError> {
    Ok(state.core.add_library_root(path).await?)
}

#[tauri::command]
pub async fn remove_library_root(
    state: State<'_, AppState>,
    path: String,
) -> Result<LibrarySnapshot, CommandError> {
    Ok(state.core.remove_library_root(path).await?)
}

#[tauri::command]
pub async fn clear_library(state: State<'_, AppState>) -> Result<LibrarySnapshot, CommandError> {
    Ok(state.core.clear_library().await?)
}

#[tauri::command]
pub async fn start_library_scan(state: State<'_, AppState>) -> Result<(), CommandError> {
    Ok(state.core.start_library_scan().await?)
}

#[tauri::command]
pub async fn set_field_mappings(
    state: State<'_, AppState>,
    mappings: Vec<LibraryFieldMapping>,
) -> Result<LibrarySnapshot, CommandError> {
    Ok(state.core.set_field_mappings(mappings).await)
}

#[tauri::command]
pub async fn set_catalog_rules(
    state: State<'_, AppState>,
    rules: Vec<CatalogRule>,
) -> Result<LibrarySnapshot, CommandError> {
    Ok(state.core.set_catalog_rules(rules).await?)
}

#[tauri::command]
pub async fn read_track_raw_tags(
    state: State<'_, AppState>,
    path: String,
) -> Result<BTreeMap<String, Vec<String>>, CommandError> {
    Ok(state.core.read_track_raw_tags(path).await?)
}

#[tauri::command]
pub async fn export_field_to_tag(
    state: State<'_, AppState>,
    request: FieldExportRequest,
) -> Result<LibrarySnapshot, CommandError> {
    Ok(state.core.export_field_to_tag(request).await?)
}

#[tauri::command]
pub async fn edit_track_tags(
    state: State<'_, AppState>,
    request: TrackTagEditRequest,
) -> Result<LibrarySnapshot, CommandError> {
    Ok(state.core.edit_track_tags(request).await?)
}
