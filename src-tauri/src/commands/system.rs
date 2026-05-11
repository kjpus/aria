use std::path::{Path, PathBuf};

use aria_domain::AppBootstrap;
use tauri::State;

use crate::{error::CommandError, AppState};

#[tauri::command]
pub async fn bootstrap(state: State<'_, AppState>) -> Result<AppBootstrap, CommandError> {
    Ok(state.core.bootstrap().await)
}

#[tauri::command]
pub async fn pick_directory() -> Result<Option<String>, CommandError> {
    #[cfg(target_os = "windows")]
    {
        tokio::task::spawn_blocking(|| {
            let script = r#"
Add-Type -AssemblyName System.Windows.Forms | Out-Null
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'Select a music library directory'
$dialog.ShowNewFolderButton = $false
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
  [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
  Write-Output $dialog.SelectedPath
}
"#;

            let output = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-STA", "-Command", script])
                .output()
                .map_err(|error| CommandError::Message(error.to_string()))?;

            if !output.status.success() {
                let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
                return Err(CommandError::Message(if message.is_empty() {
                    "Directory selection failed.".into()
                } else {
                    message
                }));
            }

            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok((!path.is_empty()).then_some(path))
        })
        .await
        .map_err(|error| CommandError::Message(error.to_string()))?
    }

    #[cfg(not(target_os = "windows"))]
    {
        Err(CommandError::Message(
            "Directory selection is not supported on this platform yet.".into(),
        ))
    }
}

#[tauri::command]
pub fn open_directory(path: String) -> Result<(), CommandError> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(path)
            .spawn()
            .map_err(|error| CommandError::Message(error.to_string()))?;
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err(CommandError::Message(
            "Opening media folders is not supported on this platform yet.".into(),
        ))
    }
}

#[tauri::command]
pub fn show_in_explorer(path: String) -> Result<(), CommandError> {
    #[cfg(target_os = "windows")]
    {
        let explorer_args = explorer_select_arguments(&path);

        std::process::Command::new("explorer.exe")
            .args(explorer_args)
            .spawn()
            .map_err(|error| CommandError::Message(error.to_string()))?;
        Ok(())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        Err(CommandError::Message(
            "Showing media files in the system file browser is not supported on this platform yet."
                .into(),
        ))
    }
}

#[tauri::command]
pub fn debug_log(message: String) {
    #[cfg(debug_assertions)]
    eprintln!("[aria-ui] {message}");

    #[cfg(not(debug_assertions))]
    let _ = message;
}

#[cfg(target_os = "windows")]
fn explorer_select_arguments(path: &str) -> [String; 2] {
    let normalized = normalize_explorer_path(path);
    ["/select,".to_string(), normalized.display().to_string()]
}

#[cfg(target_os = "windows")]
fn normalize_explorer_path(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| Path::new(path).to_path_buf())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use super::explorer_select_arguments;

    #[cfg(target_os = "windows")]
    #[test]
    fn explorer_select_argument_splits_switch_and_path() {
        let arguments = explorer_select_arguments(r"D:\Music Library\Disc 1\track 01.m4a");

        assert_eq!(arguments, ["/select,".to_string(), r"D:\Music Library\Disc 1\track 01.m4a".to_string()]);
    }
}
