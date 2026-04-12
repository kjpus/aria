use aria_domain::{CatalogPatternRule, LibraryFieldMapping, LibrarySnapshot};
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
    rules: Vec<CatalogPatternRule>,
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
