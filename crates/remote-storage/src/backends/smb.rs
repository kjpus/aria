use std::path::{Path, PathBuf};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::backends::filesystem::FilesystemBackend;
use crate::error::RemoteStorageError;
use crate::traits::RemoteStorageBackend;
use crate::types::{RemoteFileRef, StorageStatus, StorageUsage, TransferProgress};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmbConfig {
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub share: String,
    #[serde(default, rename = "sharePath")]
    pub share_path: Option<String>,
    #[serde(default, rename = "share_path")]
    pub share_path_snake: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

impl SmbConfig {
    pub fn get_share_path(&self) -> Option<&str> {
        self.share_path
            .as_deref()
            .or(self.share_path_snake.as_deref())
    }
}

#[derive(Debug, Clone)]
pub struct SmbBackend {
    config: SmbConfig,
    share_root: String,
    unc_path: PathBuf,
    fs_backend: FilesystemBackend,
}

impl SmbBackend {
    pub fn new(mut config: SmbConfig) -> Self {
        if config.server.trim().is_empty() {
            if let Some(path) = config.get_share_path().map(|s| s.to_string()) {
                let trimmed = path
                    .trim()
                    .trim_start_matches("smb://")
                    .trim_start_matches(['\\', '/']);
                if let Some((srv, sh)) = trimmed.split_once(['\\', '/']) {
                    config.server = srv.to_string();
                    config.share = sh.trim_end_matches(['\\', '/']).to_string();
                } else {
                    config.server = trimmed.to_string();
                }
            }
        }

        if config.port.is_none() && config.server.contains(':') {
            if let Some((host, port_str)) = config.server.split_once(':') {
                if let Ok(p) = port_str.parse::<u16>() {
                    config.port = Some(p);
                    config.server = host.to_string();
                }
            }
        }

        let server = config.server.trim().trim_start_matches(['\\', '/']);
        let share_full = config.share.trim().trim_matches(['\\', '/']);

        // Split share_full into primary share name (e.g. "usbshare1") and optional subfolder (e.g. "Music/Classical")
        let (primary_share, _) = match share_full.split_once(['\\', '/']) {
            Some((first, rest)) => (first, Some(rest)),
            None => (share_full, None),
        };

        let share_root = if primary_share.is_empty() {
            format!("\\\\{}", server)
        } else {
            format!("\\\\{}\\{}", server, primary_share)
        };

        let unc_str = if share_full.is_empty() {
            format!("\\\\{}", server)
        } else {
            format!("\\\\{}\\{}", server, share_full)
        };
        let unc_path = PathBuf::from(&unc_str);
        let fs_backend = FilesystemBackend::new(&unc_path);

        Self {
            config,
            share_root,
            unc_path,
            fs_backend,
        }
    }

    pub fn unc_path(&self) -> &Path {
        &self.unc_path
    }

    pub fn share_root(&self) -> &str {
        &self.share_root
    }

    /// Authenticates with the remote SMB server using in-app credentials.
    /// On Windows, this utilizes `WNetAddConnection2W` to authenticate the UNC share
    /// into the process network session without requiring manual Windows Explorer setup.
    pub async fn authenticate(&self) -> Result<(), RemoteStorageError> {
        let port = self.config.port.unwrap_or(445);
        let addr = format!("{}:{}", self.config.server.trim(), port);

        // 1. TCP connectivity check
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            TcpStream::connect(&addr),
        )
        .await
        .map_err(|_| RemoteStorageError::ConnectionFailed(format!("Connection to {addr} timed out")))?
        .map_err(|e| RemoteStorageError::ConnectionFailed(format!("Cannot reach {addr}: {e}")))?;

        // 2. Windows-native in-app authentication (connecting to the root share)
        #[cfg(target_os = "windows")]
        {
            let share_root = self.share_root.clone();
            let username = self.config.username.clone();
            let password = self.config.password.clone();

            tokio::task::spawn_blocking(move || {
                windows_connect_net_resource(&share_root, username.as_deref(), password.as_deref())
            })
            .await
            .map_err(|e| RemoteStorageError::ProviderError(format!("Task join error: {e}")))?
            .map_err(RemoteStorageError::AuthFailed)?;
        }

        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn windows_connect_net_resource(
    remote_name: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct NETRESOURCEW {
        dw_scope: u32,
        dw_type: u32,
        dw_display_type: u32,
        dw_usage: u32,
        lp_local_name: *mut u16,
        lp_remote_name: *mut u16,
        lp_comment: *mut u16,
        lp_provider: *mut u16,
    }

    const RESOURCETYPE_DISK: u32 = 0x00000001;

    #[link(name = "mpr")]
    extern "system" {
        fn WNetAddConnection2W(
            lp_net_resource: *const NETRESOURCEW,
            lp_password: *const u16,
            lp_user_name: *const u16,
            dw_flags: u32,
        ) -> u32;
    }

    let mut wide_remote: Vec<u16> = OsStr::new(remote_name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let wide_user: Option<Vec<u16>> = username.map(|u| {
        OsStr::new(u)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    });

    let wide_pass: Option<Vec<u16>> = password.map(|p| {
        OsStr::new(p)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    });

    let net_res = NETRESOURCEW {
        dw_scope: 0,
        dw_type: RESOURCETYPE_DISK,
        dw_display_type: 0,
        dw_usage: 0,
        lp_local_name: std::ptr::null_mut(),
        lp_remote_name: wide_remote.as_mut_ptr(),
        lp_comment: std::ptr::null_mut(),
        lp_provider: std::ptr::null_mut(),
    };

    let user_ptr = wide_user
        .as_ref()
        .map(|v| v.as_ptr())
        .unwrap_or(std::ptr::null());
    let pass_ptr = wide_pass
        .as_ref()
        .map(|v| v.as_ptr())
        .unwrap_or(std::ptr::null());

    let result = unsafe { WNetAddConnection2W(&net_res, pass_ptr, user_ptr, 0) };

    // Error code 0 = NO_ERROR, 1219 = ERROR_SESSION_CREDENTIAL_CONFLICT (already connected with creds)
    if result == 0 || result == 1219 {
        Ok(())
    } else {
        Err(format!("Windows network connection failed with code {result}"))
    }
}

#[async_trait]
impl RemoteStorageBackend for SmbBackend {
    fn provider_id(&self) -> &'static str {
        "smb"
    }

    async fn test_connection(&self) -> Result<StorageStatus, RemoteStorageError> {
        if let Err(e) = self.authenticate().await {
            return Ok(StorageStatus {
                is_connected: false,
                message: Some(format!("SMB connection failed: {e}")),
                storage_usage: None,
            });
        }

        self.fs_backend.test_connection().await
    }

    async fn get_storage_usage(&self) -> Result<StorageUsage, RemoteStorageError> {
        self.fs_backend.get_storage_usage().await
    }

    async fn ensure_directory(&self, path: &str) -> Result<String, RemoteStorageError> {
        self.fs_backend.ensure_directory(path).await
    }

    async fn upload_file(
        &self,
        local_path: &Path,
        remote_path: &str,
        progress: Option<mpsc::Sender<TransferProgress>>,
    ) -> Result<RemoteFileRef, RemoteStorageError> {
        self.fs_backend
            .upload_file(local_path, remote_path, progress)
            .await
    }

    async fn write_bytes(&self, remote_path: &str, data: &[u8]) -> Result<(), RemoteStorageError> {
        self.fs_backend.write_bytes(remote_path, data).await
    }

    async fn read_bytes(&self, remote_path: &str) -> Result<Vec<u8>, RemoteStorageError> {
        self.fs_backend.read_bytes(remote_path).await
    }

    async fn delete_file(&self, remote_path: &str) -> Result<(), RemoteStorageError> {
        self.fs_backend.delete_file(remote_path).await
    }

    async fn delete_directory(&self, remote_path: &str) -> Result<(), RemoteStorageError> {
        self.fs_backend.delete_directory(remote_path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smb_unc_path_formatting() {
        let config = SmbConfig {
            server: "192.168.1.50".to_string(),
            share: "ClassicalMusic".to_string(),
            share_path: None,
            share_path_snake: None,
            username: Some("aria_user".to_string()),
            password: Some("secret".to_string()),
            domain: None,
            port: Some(445),
        };
        let backend = SmbBackend::new(config);
        assert_eq!(
            backend.unc_path().to_string_lossy(),
            r"\\192.168.1.50\ClassicalMusic"
        );
    }

    #[test]
    fn test_smb_share_path_deserialization_and_parsing() {
        let json = r#"{"share_path": "\\\\192.168.1.50\\ClassicalMusic", "username": "user"}"#;
        let config: SmbConfig = serde_json::from_str(json).expect("should deserialize share_path");
        let backend = SmbBackend::new(config);
        assert_eq!(
            backend.unc_path().to_string_lossy(),
            r"\\192.168.1.50\ClassicalMusic"
        );
        assert_eq!(backend.config.server, "192.168.1.50");
        assert_eq!(backend.config.share, "ClassicalMusic");
    }

    #[test]
    fn test_smb_subfolder_on_share_handling() {
        let json = r#"{"share_path": "\\\\nas\\usbshare1\\Music\\Classical", "username": "user"}"#;
        let config: SmbConfig = serde_json::from_str(json).expect("should deserialize share_path with subfolder");
        let backend = SmbBackend::new(config);
        assert_eq!(
            backend.unc_path().to_string_lossy(),
            r"\\nas\usbshare1\Music\Classical"
        );
        assert_eq!(backend.share_root(), r"\\nas\usbshare1");
    }

    #[test]
    fn test_smb_both_share_path_and_share_path_snake() {
        let json = r#"{"server":"nas","share":"usbshare1\\aria","sharePath":"\\\\nas\\usbshare1\\aria","share_path":"\\\\nas\\usbshare1\\aria"}"#;
        let config: SmbConfig = serde_json::from_str(json).expect("should tolerate both sharePath and share_path");
        let backend = SmbBackend::new(config);
        assert_eq!(
            backend.unc_path().to_string_lossy(),
            r"\\nas\usbshare1\aria"
        );
        assert_eq!(backend.share_root(), r"\\nas\usbshare1");
    }
}
