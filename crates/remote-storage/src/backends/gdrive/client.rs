use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{header, Client, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::backends::gdrive::auth::{refresh_access_token, GoogleAuthConfig, GoogleTokens};
use crate::error::RemoteStorageError;
use crate::traits::RemoteStorageBackend;
use crate::types::{RemoteFileRef, StorageStatus, StorageUsage, TransferProgress};

const DRIVE_API_BASE: &str = "https://www.googleapis.com/drive/v3";
const DRIVE_UPLOAD_BASE: &str = "https://www.googleapis.com/upload/drive/v3/files";

#[derive(Debug, Clone)]
pub struct GoogleDriveBackend {
    config: GoogleAuthConfig,
    tokens: Arc<Mutex<GoogleTokens>>,
    client: Client,
    root_folder_name: String,
    folder_id_cache: Arc<RwLock<HashMap<String, String>>>,
}

#[derive(Deserialize)]
struct DriveFileList {
    files: Vec<DriveFileEntry>,
}

#[derive(Deserialize, Serialize)]
struct DriveFileEntry {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
}

#[derive(Deserialize)]
struct DriveAbout {
    #[serde(rename = "storageQuota")]
    storage_quota: Option<DriveStorageQuota>,
}

#[derive(Deserialize)]
struct DriveStorageQuota {
    limit: Option<String>,
    usage: Option<String>,
    #[serde(rename = "usageInDrive")]
    usage_in_drive: Option<String>,
}

impl GoogleDriveBackend {
    pub fn new(
        config: GoogleAuthConfig,
        tokens: GoogleTokens,
        root_folder_name: impl Into<String>,
    ) -> Self {
        Self {
            config,
            tokens: Arc::new(Mutex::new(tokens)),
            client: Client::builder().build().unwrap_or_default(),
            root_folder_name: root_folder_name.into(),
            folder_id_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Obtains a valid Bearer token, refreshing automatically if close to expiration.
    async fn get_access_token(&self) -> Result<String, RemoteStorageError> {
        let mut tokens_guard = self.tokens.lock().await;
        if tokens_guard.is_expired() {
            if let Some(ref refresh_tok) = tokens_guard.refresh_token {
                let new_tokens = refresh_access_token(&self.config, refresh_tok).await?;
                *tokens_guard = new_tokens;
            } else {
                return Err(RemoteStorageError::AuthFailed(
                    "Token expired and no refresh token available".into(),
                ));
            }
        }
        Ok(tokens_guard.access_token.clone())
    }

    /// Resolves or creates the configured root folder on Google Drive (default "Aria").
    /// Supports nested paths like "Music/Aria" or simple folder names like "Aria".
    async fn get_or_create_root_folder(&self) -> Result<String, RemoteStorageError> {
        let cache_key = format!("__ROOT__{}", self.root_folder_name);
        {
            let cache = self.folder_id_cache.read().await;
            if let Some(id) = cache.get(&cache_key) {
                return Ok(id.clone());
            }
        }

        let segments: Vec<&str> = self
            .root_folder_name
            .split(['/', '\\'])
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        if segments.is_empty() {
            return Err(RemoteStorageError::ProviderError(
                "Google Drive root folder name cannot be empty".into(),
            ));
        }

        let token = self.get_access_token().await?;
        let mut parent_id: Option<String> = None;
        let mut accumulated_path = String::new();

        for segment in segments {
            if !accumulated_path.is_empty() {
                accumulated_path.push('/');
            }
            accumulated_path.push_str(segment);

            let segment_cache_key = format!("__ROOT__{accumulated_path}");
            let cached_id = {
                let cache = self.folder_id_cache.read().await;
                cache.get(&segment_cache_key).cloned()
            };

            let folder_id = match cached_id {
                Some(id) => id,
                None => {
                    let query = if let Some(ref pid) = parent_id {
                        format!(
                            "name = '{}' and '{}' in parents and mimeType = 'application/vnd.google-apps.folder' and trashed = false",
                            segment.replace('\'', "\\'"),
                            pid
                        )
                    } else {
                        // Top level folder: under drive.file scope, do not query 'root' in parents
                        format!(
                            "name = '{}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false",
                            segment.replace('\'', "\\'")
                        )
                    };

                    let url = format!("{DRIVE_API_BASE}/files?q={}&fields=files(id,name)", urlencoding(&query));
                    let res = self
                        .client
                        .get(&url)
                        .bearer_auth(&token)
                        .send()
                        .await
                        .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

                    let list: DriveFileList = if res.status().is_success() {
                        res.json().await.unwrap_or(DriveFileList { files: Vec::new() })
                    } else {
                        DriveFileList { files: Vec::new() }
                    };

                    let found_id = if let Some(f) = list.files.into_iter().next() {
                        f.id
                    } else {
                        let create_url = format!("{DRIVE_API_BASE}/files");
                        let mut body_map = serde_json::Map::new();
                        body_map.insert("name".to_string(), serde_json::Value::String(segment.to_string()));
                        body_map.insert(
                            "mimeType".to_string(),
                            serde_json::Value::String("application/vnd.google-apps.folder".to_string()),
                        );
                        if let Some(ref pid) = parent_id {
                            body_map.insert("parents".to_string(), serde_json::json!([pid]));
                        }

                        let create_res = self
                            .client
                            .post(&create_url)
                            .bearer_auth(&token)
                            .json(&serde_json::Value::Object(body_map))
                            .send()
                            .await
                            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

                        if !create_res.status().is_success() {
                            return Err(RemoteStorageError::ProviderError(format!(
                                "Failed to create folder '{segment}': {}",
                                create_res.status()
                            )));
                        }

                        let entry: DriveFileEntry = create_res
                            .json()
                            .await
                            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;
                        entry.id
                    };

                    let mut cache = self.folder_id_cache.write().await;
                    cache.insert(segment_cache_key, found_id.clone());
                    found_id
                }
            };

            parent_id = Some(folder_id);
        }

        let final_root_id = parent_id.ok_or_else(|| {
            RemoteStorageError::ProviderError("Failed to determine Google Drive root folder".into())
        })?;

        let mut cache = self.folder_id_cache.write().await;
        cache.insert(cache_key, final_root_id.clone());
        Ok(final_root_id)
    }

    /// Resolves a folder ID within a parent folder, creating it if it does not exist.
    async fn resolve_child_folder(
        &self,
        parent_id: &str,
        folder_name: &str,
    ) -> Result<String, RemoteStorageError> {
        let token = self.get_access_token().await?;
        let query = format!(
            "name = '{}' and '{}' in parents and mimeType = 'application/vnd.google-apps.folder' and trashed = false",
            folder_name.replace('\'', "\\'"),
            parent_id
        );

        let url = format!("{DRIVE_API_BASE}/files?q={}&fields=files(id,name)", urlencoding(&query));
        let res = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if !res.status().is_success() {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to search folder: {}",
                res.status()
            )));
        }

        let list: DriveFileList = res
            .json()
            .await
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

        if let Some(entry) = list.files.into_iter().next() {
            return Ok(entry.id);
        }

        // Create the child folder
        let create_url = format!("{DRIVE_API_BASE}/files");
        let body = serde_json::json!({
            "name": folder_name,
            "mimeType": "application/vnd.google-apps.folder",
            "parents": [parent_id]
        });

        let create_res = self
            .client
            .post(&create_url)
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if !create_res.status().is_success() {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to create folder '{folder_name}': {}",
                create_res.status()
            )));
        }

        let entry: DriveFileEntry = create_res
            .json()
            .await
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;
        Ok(entry.id)
    }

    /// Resolves a file ID within a parent folder by filename.
    async fn find_file_in_parent(
        &self,
        parent_id: &str,
        file_name: &str,
    ) -> Result<Option<String>, RemoteStorageError> {
        let token = self.get_access_token().await?;
        let query = format!(
            "name = '{}' and '{}' in parents and trashed = false",
            file_name.replace('\'', "\\'"),
            parent_id
        );

        let url = format!("{DRIVE_API_BASE}/files?q={}&fields=files(id,name)", urlencoding(&query));
        let res = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if !res.status().is_success() {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to search file: {}",
                res.status()
            )));
        }

        let list: DriveFileList = res
            .json()
            .await
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

        Ok(list.files.into_iter().next().map(|f| f.id))
    }

    /// Splits a relative path (e.g. "Beethoven/Sym9/manifest.json") into directory path and file name.
    fn split_path(remote_path: &str) -> (Option<&str>, &str) {
        let trimmed = remote_path.trim_matches(['/', '\\']);
        match trimmed.rfind(['/', '\\']) {
            Some(idx) => (Some(&trimmed[..idx]), &trimmed[idx + 1..]),
            None => (None, trimmed),
        }
    }
}

fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{:02X}", b);
            }
        }
    }
    out
}

#[async_trait]
impl RemoteStorageBackend for GoogleDriveBackend {
    fn provider_id(&self) -> &'static str {
        "google_drive"
    }

    async fn test_connection(&self) -> Result<StorageStatus, RemoteStorageError> {
        let token = match self.get_access_token().await {
            Ok(t) => t,
            Err(e) => {
                return Ok(StorageStatus {
                    is_connected: false,
                    message: Some(format!("Authentication error: {e}")),
                    storage_usage: None,
                });
            }
        };

        let url = format!("{DRIVE_API_BASE}/about?fields=storageQuota,user");
        let res = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if !res.status().is_success() {
            return Ok(StorageStatus {
                is_connected: false,
                message: Some(format!("Drive API returned status {}", res.status())),
                storage_usage: None,
            });
        }

        let about: DriveAbout = res
            .json()
            .await
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

        let usage = about.storage_quota.map(|sq| {
            let total = sq.limit.and_then(|s| s.parse::<u64>().ok());
            let used = sq
                .usage_in_drive
                .or(sq.usage)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let free = total.map(|t| t.saturating_sub(used));

            StorageUsage {
                used_bytes: used,
                total_bytes: total,
                free_bytes: free,
            }
        });

        Ok(StorageStatus {
            is_connected: true,
            message: Some("Connected to Google Drive".into()),
            storage_usage: usage,
        })
    }

    async fn get_storage_usage(&self) -> Result<StorageUsage, RemoteStorageError> {
        let status = self.test_connection().await?;
        status
            .storage_usage
            .ok_or_else(|| RemoteStorageError::ProviderError("No storage quota info".into()))
    }

    async fn ensure_directory(&self, path: &str) -> Result<String, RemoteStorageError> {
        let normalized = path.trim_matches(['/', '\\']);
        if normalized.is_empty() {
            return self.get_or_create_root_folder().await;
        }

        // Check in-memory folder cache
        {
            let cache = self.folder_id_cache.read().await;
            if let Some(id) = cache.get(normalized) {
                return Ok(id.clone());
            }
        }

        let mut current_id = self.get_or_create_root_folder().await?;
        let segments: Vec<&str> = normalized.split(['/', '\\']).filter(|s| !s.is_empty()).collect();

        let mut accumulated_path = String::new();

        for segment in segments {
            if !accumulated_path.is_empty() {
                accumulated_path.push('/');
            }
            accumulated_path.push_str(segment);

            let cached_segment_id = {
                let cache = self.folder_id_cache.read().await;
                cache.get(&accumulated_path).cloned()
            };

            let next_id = match cached_segment_id {
                Some(id) => id,
                None => {
                    let created_id = self.resolve_child_folder(&current_id, segment).await?;
                    let mut cache = self.folder_id_cache.write().await;
                    cache.insert(accumulated_path.clone(), created_id.clone());
                    created_id
                }
            };
            current_id = next_id;
        }

        Ok(current_id)
    }

    async fn upload_file(
        &self,
        local_path: &Path,
        remote_path: &str,
        progress: Option<mpsc::Sender<TransferProgress>>,
    ) -> Result<RemoteFileRef, RemoteStorageError> {
        let (dir_part, file_name) = Self::split_path(remote_path);
        let parent_id = match dir_part {
            Some(dir) => self.ensure_directory(dir).await?,
            None => self.get_or_create_root_folder().await?,
        };

        let mut file = File::open(local_path)
            .await
            .map_err(RemoteStorageError::Io)?;
        let file_len = file
            .metadata()
            .await
            .map_err(RemoteStorageError::Io)?
            .len();

        let token = self.get_access_token().await?;

        // 1. Initiate Resumable Upload Session
        let init_url = format!("{DRIVE_UPLOAD_BASE}?uploadType=resumable");
        let metadata_body = serde_json::json!({
            "name": file_name,
            "parents": [parent_id]
        });

        let init_res = self
            .client
            .post(&init_url)
            .bearer_auth(&token)
            .header("X-Upload-Content-Length", file_len.to_string())
            .header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
            .json(&metadata_body)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if !init_res.status().is_success() {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to initiate resumable upload: {}",
                init_res.status()
            )));
        }

        let session_uri = init_res
            .headers()
            .get(header::LOCATION)
            .and_then(|val| val.to_str().ok())
            .ok_or_else(|| {
                RemoteStorageError::ProviderError("Missing Location header in upload init".into())
            })?
            .to_string();

        // 2. Upload file content with chunked progress reporting
        let mut buffer = Vec::with_capacity(file_len as usize);
        file.read_to_end(&mut buffer)
            .await
            .map_err(RemoteStorageError::Io)?;

        if let Some(ref p) = progress {
            let _ = p
                .send(TransferProgress {
                    bytes_transferred: 0,
                    total_bytes: file_len,
                })
                .await;
        }

        let upload_res = self
            .client
            .put(&session_uri)
            .header(header::CONTENT_LENGTH, file_len.to_string())
            .body(buffer)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if !upload_res.status().is_success() && upload_res.status() != StatusCode::CREATED {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to complete chunk upload: {}",
                upload_res.status()
            )));
        }

        let uploaded_entry: DriveFileEntry = upload_res
            .json()
            .await
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

        if let Some(ref p) = progress {
            let _ = p
                .send(TransferProgress {
                    bytes_transferred: file_len,
                    total_bytes: file_len,
                })
                .await;
        }

        Ok(RemoteFileRef {
            remote_id: uploaded_entry.id,
            remote_path: remote_path.to_string(),
            size_bytes: file_len,
        })
    }

    async fn write_bytes(&self, remote_path: &str, data: &[u8]) -> Result<(), RemoteStorageError> {
        let (dir_part, file_name) = Self::split_path(remote_path);
        let parent_id = match dir_part {
            Some(dir) => self.ensure_directory(dir).await?,
            None => self.get_or_create_root_folder().await?,
        };

        let token = self.get_access_token().await?;

        // Check if file already exists in parent
        if let Some(existing_file_id) = self.find_file_in_parent(&parent_id, file_name).await? {
            // Overwrite existing file content
            let url = format!("{DRIVE_UPLOAD_BASE}/{existing_file_id}?uploadType=media");
            let res = self
                .client
                .patch(&url)
                .bearer_auth(&token)
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .body(data.to_vec())
                .send()
                .await
                .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

            if !res.status().is_success() {
                return Err(RemoteStorageError::ProviderError(format!(
                    "Failed to update file content: {}",
                    res.status()
                )));
            }
            return Ok(());
        }

        // Upload new file via multipart / media
        let init_url = format!("{DRIVE_UPLOAD_BASE}?uploadType=resumable");
        let metadata_body = serde_json::json!({
            "name": file_name,
            "parents": [parent_id]
        });

        let init_res = self
            .client
            .post(&init_url)
            .bearer_auth(&token)
            .header("X-Upload-Content-Length", data.len().to_string())
            .header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
            .json(&metadata_body)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if !init_res.status().is_success() {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to init bytes upload: {}",
                init_res.status()
            )));
        }

        let session_uri = init_res
            .headers()
            .get(header::LOCATION)
            .and_then(|val| val.to_str().ok())
            .ok_or_else(|| {
                RemoteStorageError::ProviderError("Missing Location header in upload init".into())
            })?;

        let upload_res = self
            .client
            .put(session_uri)
            .header(header::CONTENT_LENGTH, data.len().to_string())
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if !upload_res.status().is_success() && upload_res.status() != StatusCode::CREATED {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to upload bytes: {}",
                upload_res.status()
            )));
        }

        Ok(())
    }

    async fn read_bytes(&self, remote_path: &str) -> Result<Vec<u8>, RemoteStorageError> {
        let (dir_part, file_name) = Self::split_path(remote_path);
        let parent_id = match dir_part {
            Some(dir) => self.ensure_directory(dir).await?,
            None => self.get_or_create_root_folder().await?,
        };

        let file_id = self
            .find_file_in_parent(&parent_id, file_name)
            .await?
            .ok_or_else(|| RemoteStorageError::NotFound(remote_path.to_string()))?;

        let token = self.get_access_token().await?;
        let url = format!("{DRIVE_API_BASE}/files/{file_id}?alt=media");

        let res = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if !res.status().is_success() {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to read file from Drive: {}",
                res.status()
            )));
        }

        let bytes = res
            .bytes()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        Ok(bytes.to_vec())
    }

    async fn delete_file(&self, remote_path: &str) -> Result<(), RemoteStorageError> {
        let (dir_part, file_name) = Self::split_path(remote_path);
        let parent_id = match dir_part {
            Some(dir) => self.ensure_directory(dir).await?,
            None => self.get_or_create_root_folder().await?,
        };

        if let Some(file_id) = self.find_file_in_parent(&parent_id, file_name).await? {
            let token = self.get_access_token().await?;
            let url = format!("{DRIVE_API_BASE}/files/{file_id}");
            let res = self
                .client
                .delete(&url)
                .bearer_auth(&token)
                .send()
                .await
                .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

            if !res.status().is_success() && res.status() != StatusCode::NOT_FOUND {
                return Err(RemoteStorageError::ProviderError(format!(
                    "Failed to delete file on Drive: {}",
                    res.status()
                )));
            }
        }

        Ok(())
    }

    async fn delete_directory(&self, remote_path: &str) -> Result<(), RemoteStorageError> {
        let normalized = remote_path.trim_matches(['/', '\\']);
        if normalized.is_empty() {
            return Ok(());
        }

        let dir_id = self.ensure_directory(normalized).await?;
        let token = self.get_access_token().await?;
        let url = format!("{DRIVE_API_BASE}/files/{dir_id}");
        let res = self
            .client
            .delete(&url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if !res.status().is_success() && res.status() != StatusCode::NOT_FOUND {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to delete directory on Drive: {}",
                res.status()
            )));
        }

        let mut cache = self.folder_id_cache.write().await;
        cache.remove(normalized);

        Ok(())
    }
}
