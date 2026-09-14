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
#[serde(rename_all = "camelCase")]
pub struct SmbConfig {
    pub server: String,
    pub share: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub domain: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct SmbBackend {
    config: SmbConfig,
    unc_path: PathBuf,
    fs_backend: FilesystemBackend,
}

impl SmbBackend {
    pub fn new(config: SmbConfig) -> Self {
        let server = config.server.trim().trim_start_matches(['\\', '/']);
        let share = config.share.trim().trim_matches(['\\', '/']);
        let unc_str = format!("\\\\{}\\{}", server, share);
        let unc_path = PathBuf::from(&unc_str);
        let fs_backend = FilesystemBackend::new(&unc_path);

        Self {
            config,
            unc_path,
            fs_backend,
        }
    }

    pub fn unc_path(&self) -> &Path {
        &self.unc_path
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

        // 2. Windows-native in-app authentication
        #[cfg(target_os = "windows")]
        {
            let unc_str = self.unc_path.to_string_lossy().to_string();
            let username = self.config.username.clone();
            let password = self.config.password.clone();

            tokio::task::spawn_blocking(move || {
                windows_connect_net_resource(&unc_str, username.as_deref(), password.as_deref())
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
}
