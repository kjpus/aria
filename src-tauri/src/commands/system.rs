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
        std::process::Command::new("explorer.exe")
            .arg("/select,")
            .arg(path)
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
