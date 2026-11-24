#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use std::path::Path;
#[cfg(target_os = "windows")]
use std::path::PathBuf;

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

            use std::os::windows::process::CommandExt;
            let output = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-STA", "-Command", script])
                .creation_flags(0x08000000) // CREATE_NO_WINDOW
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

    #[cfg(target_os = "macos")]
    {
        tokio::task::spawn_blocking(|| {
            let output = std::process::Command::new("osascript")
                .args([
                    "-e",
                    r#"POSIX path of (choose folder with prompt "Select a music library directory")"#,
                ])
                .output()
                .map_err(|error| CommandError::Message(error.to_string()))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("-128") || stderr.contains("User canceled") {
                    return Ok(None);
                }
                let message = stderr.trim().to_string();
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

    #[cfg(target_os = "linux")]
    {
        tokio::task::spawn_blocking(|| {
            let output = std::process::Command::new("zenity")
                .args([
                    "--file-selection",
                    "--directory",
                    "--title=Select a music library directory",
                ])
                .output()
                .or_else(|_| {
                    std::process::Command::new("kdialog")
                        .args(["--getexistingdirectory", ".", "--title", "Select a music library directory"])
                        .output()
                })
                .map_err(|error| CommandError::Message(error.to_string()))?;

            if !output.status.success() {
                return Ok(None);
            }

            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok((!path.is_empty()).then_some(path))
        })
        .await
        .map_err(|error| CommandError::Message(error.to_string()))?
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err(CommandError::Message(
            "Directory selection is not supported on this platform yet.".into(),
        ))
    }
}

#[tauri::command]
pub fn open_directory(path: String) -> Result<(), CommandError> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        let dir_path = Path::new(&path);
        if !dir_path.is_dir() {
            return Err(CommandError::Message(
                "Target path is not an existing directory.".into(),
            ));
        }

        #[cfg(target_os = "windows")]
        let mut cmd = {
            let mut c = std::process::Command::new("explorer.exe");
            c.arg(strip_windows_verbatim_prefix(dir_path));
            c
        };

        #[cfg(target_os = "macos")]
        let mut cmd = {
            let mut c = std::process::Command::new("open");
            c.arg(dir_path);
            c
        };

        #[cfg(target_os = "linux")]
        let mut cmd = {
            let mut c = std::process::Command::new("xdg-open");
            c.arg(dir_path);
            c
        };

        cmd.spawn()
            .map_err(|error| CommandError::Message(error.to_string()))?;
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        Err(CommandError::Message(
            "Opening media folders is not supported on this platform yet.".into(),
        ))
    }
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), CommandError> {
    let parsed = url::Url::parse(&url)
        .map_err(|e| CommandError::Message(format!("Invalid URL: {e}")))?;

    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(CommandError::Message(
            "Only http and https protocols are permitted.".into(),
        ));
    }

    let safe_url = parsed.as_str();

    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "shell32")]
        extern "system" {
            fn ShellExecuteW(
                hwnd: *mut std::ffi::c_void,
                lpOperation: *const u16,
                lpFile: *const u16,
                lpParameters: *const u16,
                lpDirectory: *const u16,
                nShowCmd: i32,
            ) -> isize;
        }

        let wide_op: Vec<u16> = OsStr::new("open")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let wide_file: Vec<u16> = OsStr::new(safe_url)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        const SW_SHOWNORMAL: i32 = 1;
        let res = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                wide_op.as_ptr(),
                wide_file.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            )
        };

        if res <= 32 {
            return Err(CommandError::Message(format!(
                "Failed to open URL in browser (code {res})"
            )));
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(safe_url)
            .spawn()
            .map_err(|error| CommandError::Message(error.to_string()))?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(safe_url)
            .spawn()
            .map_err(|error| CommandError::Message(error.to_string()))?;
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = safe_url;
        Err(CommandError::Message(
            "Opening external URLs is not supported on this platform yet.".into(),
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

    #[cfg(target_os = "macos")]
    {
        let target = Path::new(&path);
        if !target.exists() {
            return Err(CommandError::Message("Target file does not exist.".into()));
        }
        std::process::Command::new("open")
            .arg("-R")
            .arg(target)
            .spawn()
            .map_err(|error| CommandError::Message(error.to_string()))?;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        let target = Path::new(&path);
        let dir = if target.is_dir() {
            target
        } else {
            target.parent().unwrap_or(target)
        };
        if !dir.is_dir() {
            return Err(CommandError::Message(
                "Target directory does not exist.".into(),
            ));
        }
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|error| CommandError::Message(error.to_string()))?;
        Ok(())
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
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
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| Path::new(path).to_path_buf());
    strip_windows_verbatim_prefix(&resolved)
}

#[cfg(target_os = "windows")]
fn strip_windows_verbatim_prefix(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{unc}"))
    } else if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use super::{explorer_select_arguments, strip_windows_verbatim_prefix};
    #[cfg(target_os = "windows")]
    use std::path::Path;

    #[cfg(target_os = "windows")]
    #[test]
    fn explorer_select_argument_splits_switch_and_path() {
        let arguments = explorer_select_arguments(r"D:\Music Library\Disc 1\track 01.m4a");

        assert_eq!(arguments, ["/select,".to_string(), r"D:\Music Library\Disc 1\track 01.m4a".to_string()]);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn strip_windows_verbatim_prefix_handles_drive_and_unc_paths() {
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"\\?\D:\Music\01.flac")),
            Path::new(r"D:\Music\01.flac")
        );
        assert_eq!(
            strip_windows_verbatim_prefix(Path::new(r"\\?\UNC\nas\music\01.flac")),
            Path::new(r"\\nas\music\01.flac")
        );
    }
}
