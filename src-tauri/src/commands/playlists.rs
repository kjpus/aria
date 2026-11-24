#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
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

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
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

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
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
pub async fn reorder_playlist_tracks(
    state: State<'_, AppState>,
    playlist_id: String,
    track_ids: Vec<String>,
) -> Result<PlaylistSnapshot, CommandError> {
    Ok(state
        .core
        .reorder_playlist_tracks(playlist_id, track_ids)
        .await?)
}

#[tauri::command]
pub async fn reorder_playlists(
    state: State<'_, AppState>,
    playlist_ids: Vec<String>,
) -> Result<PlaylistSnapshot, CommandError> {
    Ok(state
        .core
        .reorder_playlists(playlist_ids)
        .await?)
}

#[tauri::command]
pub async fn pick_playlist_file() -> Result<Option<String>, CommandError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        tokio::task::spawn_blocking(pick_import_playlist_path)
            .await
            .map_err(|error| CommandError::Message(error.to_string()))?
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
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

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn sanitize_suggested_filename(suggested_name: &str) -> String {
    suggested_name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(target_os = "windows")]
fn pick_save_playlist_path(suggested_name: &str) -> Result<Option<String>, CommandError> {
    let clean_name = sanitize_suggested_filename(suggested_name);
    let escaped_name = if clean_name.is_empty() {
        "playlist.m3u".to_string()
    } else {
        clean_name.replace('\'', "''")
    };
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

#[cfg(target_os = "macos")]
fn pick_save_playlist_path(suggested_name: &str) -> Result<Option<String>, CommandError> {
    let clean_name = sanitize_suggested_filename(suggested_name);
    let default_name = if clean_name.is_empty() {
        "playlist.m3u".to_string()
    } else if clean_name.ends_with(".m3u") {
        clean_name
    } else {
        format!("{clean_name}.m3u")
    };
    let script = format!(
        r#"POSIX path of (choose file name with prompt "Export playlist" default name "{}")"#,
        default_name.replace('"', "")
    );
    let output = std::process::Command::new("osascript")
        .args(["-e", &script])
        .output()
        .map_err(|error| CommandError::Message(error.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-128") || stderr.contains("User canceled") {
            return Ok(None);
        }
        return Err(CommandError::Message(stderr.trim().to_string()));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!path.is_empty()).then_some(path))
}

#[cfg(target_os = "linux")]
fn pick_save_playlist_path(suggested_name: &str) -> Result<Option<String>, CommandError> {
    let clean_name = sanitize_suggested_filename(suggested_name);
    let default_name = if clean_name.is_empty() {
        "playlist.m3u".to_string()
    } else if clean_name.ends_with(".m3u") {
        clean_name
    } else {
        format!("{clean_name}.m3u")
    };
    let output = std::process::Command::new("zenity")
        .args([
            "--file-selection",
            "--save",
            "--confirm-overwrite",
            "--title=Export playlist",
            &format!("--filename={default_name}"),
            "--file-filter=M3U Playlist | *.m3u",
        ])
        .output()
        .or_else(|_| {
            std::process::Command::new("kdialog")
                .args(["--getsavefilename", &default_name, "*.m3u|M3U Playlist"])
                .output()
        })
        .map_err(|error| CommandError::Message(error.to_string()))?;
    if !output.status.success() {
        return Ok(None);
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

#[cfg(target_os = "macos")]
fn pick_import_playlist_path() -> Result<Option<String>, CommandError> {
    let output = std::process::Command::new("osascript")
        .args([
            "-e",
            r#"POSIX path of (choose file with prompt "Import Playlist" of type {"m3u", "m3u8", "pls"})"#,
        ])
        .output()
        .map_err(|error| CommandError::Message(error.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("-128") || stderr.contains("User canceled") {
            return Ok(None);
        }
        return Err(CommandError::Message(stderr.trim().to_string()));
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!path.is_empty()).then_some(path))
}

#[cfg(target_os = "linux")]
fn pick_import_playlist_path() -> Result<Option<String>, CommandError> {
    let output = std::process::Command::new("zenity")
        .args([
            "--file-selection",
            "--title=Import Playlist",
            "--file-filter=Playlist files | *.m3u *.m3u8 *.pls",
        ])
        .output()
        .or_else(|_| {
            std::process::Command::new("kdialog")
                .args(["--getopenfilename", ".", "*.m3u *.m3u8 *.pls|Playlist files"])
                .output()
        })
        .map_err(|error| CommandError::Message(error.to_string()))?;
    if !output.status.success() {
        return Ok(None);
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!path.is_empty()).then_some(path))
}
