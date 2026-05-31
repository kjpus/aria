use std::fs;

use aria_domain::{PlaylistImportPreview, PlaylistSnapshot};
use tauri::State;

use crate::{error::CommandError, AppState};

#[tauri::command]
pub async fn create_playlist(
    state: State<'_, AppState>,
    name: String,
    track_ids: Vec<String>,
) -> Result<PlaylistSnapshot, CommandError> {
    Ok(state.core.create_playlist(name, track_ids).await?)
}

#[tauri::command]
pub async fn add_tracks_to_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    track_ids: Vec<String>,
) -> Result<PlaylistSnapshot, CommandError> {
    Ok(state
        .core
        .add_tracks_to_playlist(playlist_id, track_ids)
        .await?)
}

#[tauri::command]
pub async fn rename_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    name: String,
) -> Result<PlaylistSnapshot, CommandError> {
    Ok(state.core.rename_playlist(playlist_id, name).await?)
}

#[tauri::command]
pub async fn delete_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
) -> Result<PlaylistSnapshot, CommandError> {
    Ok(state.core.delete_playlist(playlist_id).await?)
}

#[tauri::command]
pub async fn regenerate_playlist_icon(
    state: State<'_, AppState>,
    playlist_id: String,
) -> Result<PlaylistSnapshot, CommandError> {
    Ok(state.core.regenerate_playlist_icon(playlist_id).await?)
}

#[tauri::command]
pub async fn export_playlist_m3u(
    state: State<'_, AppState>,
    playlist_id: String,
) -> Result<Option<String>, CommandError> {
    let (suggested_name, content) = state.core.build_playlist_m3u(playlist_id).await?;

    #[cfg(target_os = "windows")]
    {
        tokio::task::spawn_blocking(move || {
            let Some(path) = pick_save_playlist_path(&suggested_name)? else {
                return Ok(None);
            };
            fs::write(&path, content).map_err(|error| CommandError::Message(error.to_string()))?;
            Ok(Some(path))
        })
        .await
        .map_err(|error| CommandError::Message(error.to_string()))?
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = suggested_name;
        let _ = content;
        Err(CommandError::Message(
            "Playlist export is not supported on this platform yet.".into(),
        ))
    }
}

#[tauri::command]
pub async fn remove_tracks_from_playlist(
    state: State<'_, AppState>,
    playlist_id: String,
    track_ids: Vec<String>,
) -> Result<PlaylistSnapshot, CommandError> {
    Ok(state
        .core
        .remove_tracks_from_playlist(playlist_id, track_ids)
        .await?)
}

#[tauri::command]
pub async fn pick_playlist_file() -> Result<Option<String>, CommandError> {
    #[cfg(target_os = "windows")]
    {
        tokio::task::spawn_blocking(|| {
            pick_import_playlist_path()
        })
        .await
        .map_err(|error| CommandError::Message(error.to_string()))?
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(CommandError::Message(
            "Playlist import is not supported on this platform yet.".into(),
        ))
    }
}

#[tauri::command]
pub async fn get_playlist_import_preview(
    state: State<'_, AppState>,
    file_path: String,
    codepage: Option<u32>,
) -> Result<PlaylistImportPreview, CommandError> {
    Ok(state.core.get_playlist_import_preview(file_path, codepage).await?)
}

#[tauri::command]
pub async fn commit_playlist_import(
    state: State<'_, AppState>,
    file_path: String,
    name: String,
    codepage: u32,
) -> Result<PlaylistSnapshot, CommandError> {
    Ok(state.core.commit_playlist_import(file_path, name, codepage).await?)
}

#[cfg(target_os = "windows")]
fn pick_save_playlist_path(suggested_name: &str) -> Result<Option<String>, CommandError> {
    let escaped_name = suggested_name.replace('\'', "''");
    let script = format!(
        r#"
Add-Type -AssemblyName System.Windows.Forms | Out-Null
$dialog = New-Object System.Windows.Forms.SaveFileDialog
$dialog.Title = 'Export playlist'
$dialog.Filter = 'M3U Playlist (*.m3u)|*.m3u|All files (*.*)|*.*'
$dialog.DefaultExt = 'm3u'
$dialog.AddExtension = $true
$dialog.OverwritePrompt = $true
$dialog.FileName = '{escaped_name}'
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{
  [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
  Write-Output $dialog.FileName
}}
"#
    );

    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-STA", "-Command", &script])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .map_err(|error| CommandError::Message(error.to_string()))?;

    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CommandError::Message(if message.is_empty() {
            "Playlist export failed.".into()
        } else {
            message
        }));
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!path.is_empty()).then_some(path))
}

#[cfg(target_os = "windows")]
fn pick_import_playlist_path() -> Result<Option<String>, CommandError> {
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms | Out-Null
$dialog = New-Object System.Windows.Forms.OpenFileDialog
$dialog.Title = 'Import Playlist'
$dialog.Filter = 'Playlist files (*.m3u;*.m3u8;*.pls)|*.m3u;*.m3u8;*.pls|All files (*.*)|*.*'
$dialog.Multiselect = $false
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
  [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
  Write-Output $dialog.FileName
}
"#;

    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-STA", "-Command", script])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output()
        .map_err(|error| CommandError::Message(error.to_string()))?;

    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(CommandError::Message(if message.is_empty() {
            "Playlist selection failed.".into()
        } else {
            message
        }));
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!path.is_empty()).then_some(path))
}
