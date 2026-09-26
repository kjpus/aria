use std::path::PathBuf;
use std::sync::Arc;

use aria_domain::{RemoteBackendType, RemoteTarget};
use serde::{Deserialize, Serialize};

use crate::backends::gdrive::auth::{GoogleAuthConfig, GoogleTokens};
use crate::backends::gdrive::GoogleDriveBackend;
use crate::backends::smb::{SmbBackend, SmbConfig};
use crate::backends::webdav::{WebDavBackend, WebDavConfig};
use crate::backends::FilesystemBackend;
use crate::error::RemoteStorageError;
use crate::traits::RemoteStorageBackend;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleDriveStoredConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<i64>,
    #[serde(default = "default_root_folder")]
    pub root_folder_name: String,
}

fn default_root_folder() -> String {
    "Aria".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemConfig {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub root_path: Option<String>,
    #[serde(default, rename = "rootPath")]
    pub root_path_camel: Option<String>,
}

impl FilesystemConfig {
    pub fn resolved_path(&self) -> String {
        self.path
            .clone()
            .or_else(|| self.root_path.clone())
            .or_else(|| self.root_path_camel.clone())
            .unwrap_or_default()
    }
}

/// Constructs the appropriate `RemoteStorageBackend` instance from a `RemoteTarget` specification.
pub fn create_backend_for_target(
    target: &RemoteTarget,
) -> Result<Arc<dyn RemoteStorageBackend>, RemoteStorageError> {
    match target.backend_type {
        RemoteBackendType::GoogleDrive => {
            let config: GoogleDriveStoredConfig = serde_json::from_str(&target.config_json)
                .map_err(|e| RemoteStorageError::Serialization(format!("Invalid Google Drive config: {e}")))?;

            let auth_config = GoogleAuthConfig {
                client_id: config.client_id,
                client_secret: config.client_secret,
            };

            let tokens = GoogleTokens {
                access_token: config.access_token.unwrap_or_default(),
                refresh_token: config.refresh_token,
                expires_at: config.expires_at.unwrap_or(0),
            };

            Ok(Arc::new(GoogleDriveBackend::new(
                auth_config,
                tokens,
                config.root_folder_name,
            )))
        }
        RemoteBackendType::WebDav => {
            let config: WebDavConfig = serde_json::from_str(&target.config_json)
                .map_err(|e| RemoteStorageError::Serialization(format!("Invalid WebDAV config: {e}")))?;
            Ok(Arc::new(WebDavBackend::new(config)))
        }
        RemoteBackendType::Smb => {
            let config: SmbConfig = serde_json::from_str(&target.config_json)
                .map_err(|e| RemoteStorageError::Serialization(format!("Invalid SMB config: {e}")))?;
            Ok(Arc::new(SmbBackend::new(config)))
        }
        RemoteBackendType::Filesystem => {
            let path_str = if let Ok(cfg) = serde_json::from_str::<FilesystemConfig>(&target.config_json) {
                let resolved = cfg.resolved_path();
                if resolved.is_empty() {
                    target.config_json.clone()
                } else {
                    resolved
                }
            } else if let Ok(val) = serde_json::from_str::<serde_json::Value>(&target.config_json) {
                if let Some(s) = val.as_str() {
                    s.to_string()
                } else {
                    target.config_json.clone()
                }
            } else {
                target.config_json.clone()
            };
            Ok(Arc::new(FilesystemBackend::new(PathBuf::from(path_str))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filesystem_config_deserialization_tolerant() {
        // Both path and root_path present (previously caused Serde duplicate field error)
        let json_both = r#"{"path":"\\\\nas\\usbshare1\\Aria","root_path":"\\\\nas\\usbshare1\\Aria"}"#;
        let cfg: FilesystemConfig = serde_json::from_str(json_both).expect("deserialize both");
        assert_eq!(cfg.resolved_path(), r"\\nas\usbshare1\Aria");

        // Only path
        let json_path = r#"{"path":"D:\\Music"}"#;
        let cfg: FilesystemConfig = serde_json::from_str(json_path).expect("deserialize path");
        assert_eq!(cfg.resolved_path(), r"D:\Music");

        // Only root_path
        let json_root = r#"{"root_path":"D:\\Music"}"#;
        let cfg: FilesystemConfig = serde_json::from_str(json_root).expect("deserialize root_path");
        assert_eq!(cfg.resolved_path(), r"D:\Music");

        // camelCase rootPath
        let json_camel = r#"{"rootPath":"D:\\Music"}"#;
        let cfg: FilesystemConfig = serde_json::from_str(json_camel).expect("deserialize rootPath");
        assert_eq!(cfg.resolved_path(), r"D:\Music");
    }
}
