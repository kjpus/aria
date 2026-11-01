use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock};

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

/// Process-wide folder ID cache across backend instances. Keyed by `{client_id}:{root_folder_name}:{path}`.
static SHARED_FOLDER_CACHE: LazyLock<RwLock<HashMap<String, String>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

#[derive(Debug, Clone)]
pub struct GoogleDriveBackend {
    config: GoogleAuthConfig,
    tokens: Arc<Mutex<GoogleTokens>>,
    client: Client,
    root_folder_name: String,
    folder_id_cache: Arc<RwLock<HashMap<String, String>>>,
}

#[derive(Deserialize, Default)]
struct DriveFileList {
    #[serde(default)]
    files: Vec<DriveFileEntry>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct DriveFileEntry {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "mimeType")]
    mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parents: Option<Vec<String>>,
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

#[derive(Deserialize, Debug)]
struct GoogleDriveErrorBody {
    error: Option<GoogleDriveErrorObject>,
}

#[derive(Deserialize, Debug)]
struct GoogleDriveErrorObject {
    #[allow(dead_code)]
    code: Option<u16>,
    message: Option<String>,
    errors: Option<Vec<GoogleDriveErrorEntry>>,
}

#[derive(Deserialize, Debug)]
struct GoogleDriveErrorEntry {
    #[allow(dead_code)]
    message: Option<String>,
    #[allow(dead_code)]
    domain: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GoogleDriveErrorInfo {
    pub status: StatusCode,
    pub reason: Option<String>,
    pub message: String,
}

impl GoogleDriveErrorInfo {
    pub fn is_rate_limit(&self) -> bool {
        if self.status == StatusCode::TOO_MANY_REQUESTS {
            return true;
        }
        if self.status == StatusCode::FORBIDDEN {
            if let Some(ref r) = self.reason {
                let r_lower = r.to_ascii_lowercase();
                if r_lower.contains("ratelimit")
                    || r_lower.contains("userlimit")
                    || r_lower.contains("quota")
                {
                    return true;
                }
            }
            let msg_lower = self.message.to_ascii_lowercase();
            if msg_lower.contains("rate limit")
                || msg_lower.contains("user rate")
                || msg_lower.contains("quota")
            {
                return true;
            }
        }
        false
    }

    pub fn is_auth_error(&self) -> bool {
        if self.status == StatusCode::UNAUTHORIZED {
            return true;
        }
        if let Some(ref r) = self.reason {
            if r == "authError" || r == "invalidCredentials" || r == "tokenExpired" {
                return true;
            }
        }
        false
    }

    pub fn is_server_error(&self) -> bool {
        self.status.is_server_error()
    }
}

impl std::fmt::Display for GoogleDriveErrorInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(ref reason) = self.reason {
            write!(
                f,
                "Google Drive API error ({} - {}): {}",
                self.status, reason, self.message
            )
        } else {
            write!(
                f,
                "Google Drive API error ({}): {}",
                self.status, self.message
            )
        }
    }
}

pub fn parse_drive_error_body(status: StatusCode, body: &str) -> GoogleDriveErrorInfo {
    if let Ok(parsed) = serde_json::from_str::<GoogleDriveErrorBody>(body) {
        if let Some(err_obj) = parsed.error {
            let reason = err_obj
                .errors
                .and_then(|list| list.into_iter().next())
                .and_then(|e| e.reason);
            let message = err_obj.message.unwrap_or_else(|| status.to_string());
            return GoogleDriveErrorInfo {
                status,
                reason,
                message,
            };
        }
    }

    let message = if body.trim().is_empty() {
        status.to_string()
    } else {
        body.chars().take(300).collect()
    };

    GoogleDriveErrorInfo {
        status,
        reason: None,
        message,
    }
}

pub async fn parse_drive_response_error(res: reqwest::Response) -> GoogleDriveErrorInfo {
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    parse_drive_error_body(status, &body)
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
            client: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
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

    /// Explicitly forces token refresh (e.g. after 401 Unauthorized or auth error).
    async fn refresh_access_token_now(&self) -> Result<String, RemoteStorageError> {
        let mut tokens_guard = self.tokens.lock().await;
        if let Some(ref refresh_tok) = tokens_guard.refresh_token {
            let new_tokens = refresh_access_token(&self.config, refresh_tok).await?;
            let token = new_tokens.access_token.clone();
            *tokens_guard = new_tokens;
            Ok(token)
        } else {
            Err(RemoteStorageError::AuthFailed(
                "No refresh token available to refresh expired session".into(),
            ))
        }
    }

    fn cache_key(&self, path: &str) -> String {
        format!("{}:{}:{}", self.config.client_id, self.root_folder_name, path)
    }

    async fn get_cached_folder(&self, path: &str) -> Option<String> {
        // 1. Instance cache
        {
            let cache = self.folder_id_cache.read().await;
            if let Some(id) = cache.get(path) {
                return Some(id.clone());
            }
        }
        // 2. Process-wide shared cache
        let key = self.cache_key(path);
        let shared = SHARED_FOLDER_CACHE.read().await;
        let id = shared.get(&key).cloned();
        if let Some(ref found) = id {
            let mut cache = self.folder_id_cache.write().await;
            cache.insert(path.to_string(), found.clone());
        }
        id
    }

    async fn set_cached_folder(&self, path: &str, folder_id: &str) {
        {
            let mut cache = self.folder_id_cache.write().await;
            cache.insert(path.to_string(), folder_id.to_string());
        }
        let key = self.cache_key(path);
        let mut shared = SHARED_FOLDER_CACHE.write().await;
        shared.insert(key, folder_id.to_string());
    }

    async fn remove_cached_folder(&self, path: &str) {
        {
            let mut cache = self.folder_id_cache.write().await;
            cache.remove(path);
        }
        let key = self.cache_key(path);
        let mut shared = SHARED_FOLDER_CACHE.write().await;
        shared.remove(&key);
    }

    /// Sends an HTTP request with automatic token retrieval, auth-refresh retry, and exponential backoff
    /// for rate limits (429 and 403 `userRateLimitExceeded`) and transient 5xx server errors.
    async fn send_with_retry<F, Fut>(
        &self,
        op_name: &str,
        mut operation: F,
    ) -> Result<reqwest::Response, RemoteStorageError>
    where
        F: FnMut(String) -> Fut,
        Fut: std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
    {
        const MAX_RETRIES: usize = 4;
        let mut retries = 0;
        let mut token = self.get_access_token().await?;
        let mut refreshed_token = false;

        loop {
            let res = operation(token.clone())
                .await
                .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

            if res.status().is_success() || res.status() == StatusCode::CREATED {
                return Ok(res);
            }

            let err_info = parse_drive_response_error(res).await;

            // If token expired / auth error, refresh once and retry immediately
            if err_info.is_auth_error() && !refreshed_token {
                tracing::info!(op = %op_name, "Drive API auth error; refreshing access token and retrying");
                refreshed_token = true;
                match self.refresh_access_token_now().await {
                    Ok(new_tok) => {
                        token = new_tok;
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }

            // If rate-limited or transient server error, back off exponentially and retry
            if (err_info.is_rate_limit() || err_info.is_server_error()) && retries < MAX_RETRIES {
                retries += 1;
                let backoff_ms = (1000 * (1 << retries)).min(16000);
                tracing::warn!(
                    op = %op_name,
                    status = %err_info.status,
                    reason = ?err_info.reason,
                    retry = retries,
                    backoff_ms = backoff_ms,
                    "Google Drive API rate limited or server error; backing off"
                );
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                continue;
            }

            return Err(RemoteStorageError::ProviderError(format!(
                "{op_name}: {err_info}"
            )));
        }
    }

    /// Resolves or creates the configured root folder on Google Drive (default "Aria").
    /// Supports nested paths like "Music/Aria" or simple folder names like "Aria".
    async fn get_or_create_root_folder(&self) -> Result<String, RemoteStorageError> {
        let cache_key = format!("__ROOT__{}", self.root_folder_name);
        if let Some(id) = self.get_cached_folder(&cache_key).await {
            return Ok(id);
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

        let mut parent_id: Option<String> = None;
        let mut accumulated_path = String::new();

        for segment in segments {
            if !accumulated_path.is_empty() {
                accumulated_path.push('/');
            }
            accumulated_path.push_str(segment);

            let segment_cache_key = format!("__ROOT__{accumulated_path}");
            let cached_id = self.get_cached_folder(&segment_cache_key).await;

            let folder_id = match cached_id {
                Some(id) => id,
                None => {
                    let query = if let Some(ref pid) = parent_id {
                        format!(
                            "name = '{}' and '{}' in parents and mimeType = 'application/vnd.google-apps.folder' and trashed = false",
                            escape_drive_query_string(segment),
                            escape_drive_query_string(pid)
                        )
                    } else {
                        // Top level folder: under drive.file scope, do not query 'root' in parents
                        format!(
                            "name = '{}' and mimeType = 'application/vnd.google-apps.folder' and trashed = false",
                            escape_drive_query_string(segment)
                        )
                    };

                    let url = format!(
                        "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&q={}&fields=files(id,name,parents)",
                        urlencoding(&query)
                    );

                    let res = self
                        .send_with_retry(&format!("Search root segment '{segment}'"), |tok| {
                            let u = url.clone();
                            let c = self.client.clone();
                            async move { c.get(&u).bearer_auth(&tok).send().await }
                        })
                        .await?;

                    let list: DriveFileList = res.json().await.unwrap_or_default();

                    let found_id = if list.files.is_empty() {
                        let create_url = format!("{DRIVE_API_BASE}/files?supportsAllDrives=true");
                        let mut body_map = serde_json::Map::new();
                        body_map.insert("name".to_string(), serde_json::Value::String(segment.to_string()));
                        body_map.insert(
                            "mimeType".to_string(),
                            serde_json::Value::String("application/vnd.google-apps.folder".to_string()),
                        );
                        if let Some(ref pid) = parent_id {
                            body_map.insert("parents".to_string(), serde_json::json!([pid]));
                        }

                        let body = serde_json::Value::Object(body_map);
                        let create_res = self
                            .send_with_retry(&format!("Failed to create folder '{segment}'"), |tok| {
                                let u = create_url.clone();
                                let b = body.clone();
                                let c = self.client.clone();
                                async move { c.post(&u).bearer_auth(&tok).json(&b).send().await }
                            })
                            .await?;

                        let entry: DriveFileEntry = create_res
                            .json()
                            .await
                            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;
                        entry.id
                    } else if list.files.len() == 1 {
                        list.files[0].id.clone()
                    } else {
                        tracing::info!(
                            folder_count = list.files.len(),
                            segment = %segment,
                            "Multiple folders share name on Drive; disambiguating"
                        );

                        // Check if aria-library-index.json exists anywhere in Drive to identify the true root folder
                        let master_q = "name = 'aria-library-index.json' and trashed = false";
                        let master_url = format!(
                            "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&q={}&fields=files(id,name,parents)",
                            urlencoding(master_q)
                        );
                        let mut resolved_via_index = None;
                        if let Ok(res) = self
                            .send_with_retry("Locate master index", |tok| {
                                let u = master_url.clone();
                                let c = self.client.clone();
                                async move { c.get(&u).bearer_auth(&tok).send().await }
                            })
                            .await
                        {
                            if let Ok(m_list) = res.json::<DriveFileList>().await {
                                if let Some(m_file) = m_list.files.into_iter().next() {
                                    if let Some(p_id) = m_file.parents.and_then(|p| p.into_iter().next()) {
                                        if list.files.iter().any(|f| f.id == p_id) {
                                            tracing::info!(root_id = %p_id, "Root folder identified via aria-library-index.json parent");
                                            resolved_via_index = Some(p_id);
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(p_id) = resolved_via_index {
                            self.set_cached_folder(&segment_cache_key, &p_id).await;
                            return Ok(p_id);
                        }

                        // Multiple folders share this name (e.g. re-created root or duplicate folders on Drive).
                        // Disambiguate by checking which candidate actually contains 'aria-library-index.json' or contents.
                        let mut selected_id = None;
                        for cand in &list.files {
                            let chk_q = format!(
                                "name = 'aria-library-index.json' and '{}' in parents and trashed = false",
                                escape_drive_query_string(&cand.id)
                            );
                            let chk_url = format!(
                                "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&q={}&fields=files(id)",
                                urlencoding(&chk_q)
                            );
                            if let Ok(c_res) = self
                                .send_with_retry("Check candidate index", |tok| {
                                    let u = chk_url.clone();
                                    let c = self.client.clone();
                                    async move { c.get(&u).bearer_auth(&tok).send().await }
                                })
                                .await
                            {
                                if let Ok(c_list) = c_res.json::<DriveFileList>().await {
                                    if !c_list.files.is_empty() {
                                        selected_id = Some(cand.id.clone());
                                        break;
                                    }
                                }
                            }
                        }
                        if selected_id.is_none() {
                            for cand in &list.files {
                                let chk_q = format!(
                                    "'{}' in parents and trashed = false",
                                    escape_drive_query_string(&cand.id)
                                );
                                let chk_url = format!(
                                    "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&q={}&fields=files(id)&pageSize=1",
                                    urlencoding(&chk_q)
                                );
                                if let Ok(c_res) = self
                                    .send_with_retry("Check candidate children", |tok| {
                                        let u = chk_url.clone();
                                        let c = self.client.clone();
                                        async move { c.get(&u).bearer_auth(&tok).send().await }
                                    })
                                    .await
                                {
                                    if let Ok(c_list) = c_res.json::<DriveFileList>().await {
                                        if !c_list.files.is_empty() {
                                            selected_id = Some(cand.id.clone());
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        let res_id = selected_id.unwrap_or_else(|| list.files[0].id.clone());
                        tracing::info!(segment = %segment, chosen_id = %res_id, "Disambiguated folder");
                        res_id
                    };

                    self.set_cached_folder(&segment_cache_key, &found_id).await;
                    found_id
                }
            };

            parent_id = Some(folder_id);
        }

        let final_root_id = parent_id.ok_or_else(|| {
            RemoteStorageError::ProviderError("Failed to determine Google Drive root folder".into())
        })?;

        self.set_cached_folder(&cache_key, &final_root_id).await;
        Ok(final_root_id)
    }

    /// Resolves a folder ID within a parent folder, creating it if it does not exist.
    async fn resolve_child_folder(
        &self,
        parent_id: &str,
        folder_name: &str,
    ) -> Result<String, RemoteStorageError> {
        let query = format!(
            "name = '{}' and '{}' in parents and mimeType = 'application/vnd.google-apps.folder' and trashed = false",
            escape_drive_query_string(folder_name),
            escape_drive_query_string(parent_id)
        );

        let url = format!(
            "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&q={}&fields=files(id,name)",
            urlencoding(&query)
        );

        let res = self
            .send_with_retry(&format!("Failed to search folder '{folder_name}'"), |tok| {
                let u = url.clone();
                let c = self.client.clone();
                async move { c.get(&u).bearer_auth(&tok).send().await }
            })
            .await?;

        let list: DriveFileList = res
            .json()
            .await
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

        if list.files.len() == 1 {
            return Ok(list.files[0].id.clone());
        }

        if list.files.len() > 1 {
            // Disambiguate multiple child folders: prefer one that contains files
            for cand in &list.files {
                let chk_q = format!(
                    "'{}' in parents and trashed = false",
                    escape_drive_query_string(&cand.id)
                );
                let chk_url = format!(
                    "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&q={}&fields=files(id)&pageSize=1",
                    urlencoding(&chk_q)
                );
                if let Ok(c_res) = self
                    .send_with_retry("Disambiguate child folder", |tok| {
                        let u = chk_url.clone();
                        let c = self.client.clone();
                        async move { c.get(&u).bearer_auth(&tok).send().await }
                    })
                    .await
                {
                    if let Ok(c_list) = c_res.json::<DriveFileList>().await {
                        if !c_list.files.is_empty() {
                            return Ok(cand.id.clone());
                        }
                    }
                }
            }
            return Ok(list.files[0].id.clone());
        }

        // Fallback: Check parent folder listing before creating a duplicate folder
        let parent_query = format!(
            "'{}' in parents and mimeType = 'application/vnd.google-apps.folder' and trashed = false",
            escape_drive_query_string(parent_id)
        );
        let parent_url = format!(
            "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&pageSize=1000&q={}&fields=files(id,name)",
            urlencoding(&parent_query)
        );
        if let Ok(parent_res) = self
            .send_with_retry("List parent folder to verify existence", |tok| {
                let u = parent_url.clone();
                let c = self.client.clone();
                async move { c.get(&u).bearer_auth(&tok).send().await }
            })
            .await
        {
            if let Ok(parent_list) = parent_res.json::<DriveFileList>().await {
                if let Some(entry) = parent_list.files.into_iter().find(|f| f.name == folder_name) {
                    return Ok(entry.id);
                }
            }
        }

        // Create the child folder
        let create_url = format!("{DRIVE_API_BASE}/files?supportsAllDrives=true");
        let body = serde_json::json!({
            "name": folder_name,
            "mimeType": "application/vnd.google-apps.folder",
            "parents": [parent_id]
        });

        let create_res = self
            .send_with_retry(&format!("Failed to create folder '{folder_name}'"), |tok| {
                let u = create_url.clone();
                let b = body.clone();
                let c = self.client.clone();
                async move { c.post(&u).bearer_auth(&tok).json(&b).send().await }
            })
            .await?;

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
        let file_cache_key = format!("__FILE__:{parent_id}:{file_name}");
        if let Some(id) = self.get_cached_folder(&file_cache_key).await {
            return Ok(Some(id));
        }

        let query = format!(
            "name = '{}' and '{}' in parents and trashed = false",
            escape_drive_query_string(file_name),
            escape_drive_query_string(parent_id)
        );

        let url = format!(
            "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&q={}&fields=files(id,name)",
            urlencoding(&query)
        );

        let res = self
            .send_with_retry("Failed to search file", |tok| {
                let u = url.clone();
                let c = self.client.clone();
                async move { c.get(&u).bearer_auth(&tok).send().await }
            })
            .await?;

        let list: DriveFileList = res
            .json()
            .await
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

        if let Some(entry) = list.files.into_iter().next() {
            self.set_cached_folder(&file_cache_key, &entry.id).await;
            return Ok(Some(entry.id));
        }

        // Fallback: If exact query returned no matches (e.g. quote escaping or indexing lag in Google Drive search),
        // query all files directly in this parent folder and match in memory.
        let parent_query = format!("'{}' in parents and trashed = false", escape_drive_query_string(parent_id));
        let parent_url = format!(
            "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&pageSize=1000&q={}&fields=files(id,name)",
            urlencoding(&parent_query)
        );

        if let Ok(parent_res) = self
            .send_with_retry("List parent files fallback", |tok| {
                let u = parent_url.clone();
                let c = self.client.clone();
                async move { c.get(&u).bearer_auth(&tok).send().await }
            })
            .await
        {
            if let Ok(parent_list) = parent_res.json::<DriveFileList>().await {
                // Cache all files found in this parent to avoid repeat lookups
                for f in &parent_list.files {
                    let key = format!("__FILE__:{parent_id}:{}", f.name);
                    self.set_cached_folder(&key, &f.id).await;
                }

                // 1. Exact match
                if let Some(entry) = parent_list.files.iter().find(|f| f.name == file_name) {
                    return Ok(Some(entry.id.clone()));
                }
                // 2. Case-insensitive or normalized match (quotes, apostrophes)
                let normalize = |s: &str| -> String {
                    s.replace(['\u{2018}', '\u{2019}', '`'], "'")
                        .replace(['\u{201C}', '\u{201D}'], "\"")
                        .to_lowercase()
                };
                let norm_target = normalize(file_name);
                if let Some(entry) = parent_list.files.iter().find(|f| normalize(&f.name) == norm_target) {
                    self.set_cached_folder(&file_cache_key, &entry.id).await;
                    return Ok(Some(entry.id.clone()));
                }
            }
        }

        Ok(None)
    }

    async fn get_file_name_by_id(&self, file_id: &str) -> Result<String, RemoteStorageError> {
        let cache_key = format!("__NAME__{file_id}");
        if let Some(name) = self.get_cached_folder(&cache_key).await {
            return Ok(name);
        }

        let url = format!("{DRIVE_API_BASE}/files/{file_id}?supportsAllDrives=true&fields=name");
        let res = self
            .send_with_retry("Get file/folder name", |tok| {
                let u = url.clone();
                let c = self.client.clone();
                async move { c.get(&u).bearer_auth(&tok).send().await }
            })
            .await?;

        let entry: DriveFileEntry = res
            .json()
            .await
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

        self.set_cached_folder(&cache_key, &entry.name).await;
        Ok(entry.name)
    }

    /// Resolves a file ID by filename anywhere in the Google Drive target (e.g. if the directory path moved or changed).
    /// Uses dir_hint to disambiguate among candidate files with the same filename.
    async fn find_file_by_name(
        &self,
        file_name: &str,
        dir_hint: Option<&str>,
    ) -> Result<Option<String>, RemoteStorageError> {
        let query = format!(
            "name = '{}' and mimeType != 'application/vnd.google-apps.folder' and trashed = false",
            escape_drive_query_string(file_name)
        );

        let url = format!(
            "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&q={}&fields=files(id,name,parents)",
            urlencoding(&query)
        );

        let res = self
            .send_with_retry("Failed to search file by name", |tok| {
                let u = url.clone();
                let c = self.client.clone();
                async move { c.get(&u).bearer_auth(&tok).send().await }
            })
            .await?;

        let list: DriveFileList = res
            .json()
            .await
            .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

        if list.files.is_empty() {
            return Ok(None);
        }

        if list.files.len() == 1 {
            return Ok(Some(list.files[0].id.clone()));
        }

        // Multiple candidates exist with this filename: disambiguate using dir_hint
        if let Some(hint) = dir_hint {
            let hint_lower = hint.to_lowercase();
            let hint_words: Vec<&str> = hint_lower
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() >= 2)
                .collect();

            let mut best_id = None;
            let mut best_score = 0usize;

            for file in &list.files {
                let mut score = 0usize;
                if let Some(ref parents) = file.parents {
                    for parent_id in parents {
                        if let Ok(parent_name) = self.get_file_name_by_id(parent_id).await {
                            let p_lower = parent_name.to_lowercase();
                            for word in &hint_words {
                                if p_lower.contains(word) {
                                    score += word.len();
                                }
                            }
                        }
                    }
                }
                if score > best_score {
                    best_score = score;
                    best_id = Some(file.id.clone());
                }
            }

            if let Some(chosen) = best_id {
                tracing::info!(file_name, chosen = %chosen, best_score, "Disambiguated remote file by parent folder name");
                return Ok(Some(chosen));
            }
        }

        Ok(Some(list.files[0].id.clone()))
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

fn escape_drive_query_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
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
            let err_info = parse_drive_response_error(res).await;
            return Ok(StorageStatus {
                is_connected: false,
                message: Some(format!("{err_info}")),
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

        // Check shared or in-memory folder cache
        if let Some(id) = self.get_cached_folder(normalized).await {
            return Ok(id);
        }

        let mut current_id = self.get_or_create_root_folder().await?;
        let segments: Vec<&str> = normalized.split(['/', '\\']).filter(|s| !s.is_empty()).collect();

        let mut accumulated_path = String::new();

        for segment in segments {
            if !accumulated_path.is_empty() {
                accumulated_path.push('/');
            }
            accumulated_path.push_str(segment);

            let cached_segment_id = self.get_cached_folder(&accumulated_path).await;

            let next_id = match cached_segment_id {
                Some(id) => id,
                None => {
                    let created_id = self.resolve_child_folder(&current_id, segment).await?;
                    self.set_cached_folder(&accumulated_path, &created_id).await;
                    created_id
                }
            };
            current_id = next_id;
        }

        self.set_cached_folder(normalized, &current_id).await;
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

        // 1. Initiate Resumable Upload Session
        let init_url = format!("{DRIVE_UPLOAD_BASE}?uploadType=resumable&supportsAllDrives=true");
        let metadata_body = serde_json::json!({
            "name": file_name,
            "parents": [parent_id]
        });

        let init_res = self
            .send_with_retry("Failed to initiate resumable upload", |token| {
                let u = init_url.clone();
                let m = metadata_body.clone();
                let c = self.client.clone();
                async move {
                    c.post(&u)
                        .bearer_auth(&token)
                        .header("X-Upload-Content-Length", file_len.to_string())
                        .header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
                        .json(&m)
                        .send()
                        .await
                }
            })
            .await?;

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

        let mut retries = 0;
        let upload_res = loop {
            let res = self
                .client
                .put(&session_uri)
                .header(header::CONTENT_LENGTH, file_len.to_string())
                .body(buffer.clone())
                .send()
                .await
                .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

            if res.status().is_success() || res.status() == StatusCode::CREATED {
                break res;
            }

            let err_info = parse_drive_response_error(res).await;
            if (err_info.is_rate_limit() || err_info.is_server_error()) && retries < 4 {
                retries += 1;
                let backoff_ms = (1000 * (1 << retries)).min(16000);
                tracing::warn!(
                    file = %file_name,
                    status = %err_info.status,
                    reason = ?err_info.reason,
                    retry = retries,
                    backoff_ms = backoff_ms,
                    "Upload chunk rate limited or server error; backing off"
                );
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                continue;
            }

            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to complete chunk upload: {err_info}"
            )));
        };

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

        // Check if file already exists in parent
        if let Some(existing_file_id) = self.find_file_in_parent(&parent_id, file_name).await? {
            // Overwrite existing file content
            let url = format!("{DRIVE_UPLOAD_BASE}/{existing_file_id}?uploadType=media&supportsAllDrives=true");
            let data_vec = data.to_vec();
            let _ = self
                .send_with_retry("Failed to update file content", |token| {
                    let u = url.clone();
                    let d = data_vec.clone();
                    let c = self.client.clone();
                    async move {
                        c.patch(&u)
                            .bearer_auth(&token)
                            .header(header::CONTENT_TYPE, "application/octet-stream")
                            .body(d)
                            .send()
                            .await
                    }
                })
                .await?;

            return Ok(());
        }

        // Upload new file via resumable upload
        let init_url = format!("{DRIVE_UPLOAD_BASE}?uploadType=resumable&supportsAllDrives=true");
        let metadata_body = serde_json::json!({
            "name": file_name,
            "parents": [parent_id]
        });

        let init_res = self
            .send_with_retry("Failed to init bytes upload", |token| {
                let u = init_url.clone();
                let m = metadata_body.clone();
                let c = self.client.clone();
                let len_str = data.len().to_string();
                async move {
                    c.post(&u)
                        .bearer_auth(&token)
                        .header("X-Upload-Content-Length", len_str)
                        .header(header::CONTENT_TYPE, "application/json; charset=UTF-8")
                        .json(&m)
                        .send()
                        .await
                }
            })
            .await?;

        let session_uri = init_res
            .headers()
            .get(header::LOCATION)
            .and_then(|val| val.to_str().ok())
            .ok_or_else(|| {
                RemoteStorageError::ProviderError("Missing Location header in upload init".into())
            })?
            .to_string();

        let mut retries = 0;
        let data_vec = data.to_vec();
        loop {
            let res = self
                .client
                .put(&session_uri)
                .header(header::CONTENT_LENGTH, data_vec.len().to_string())
                .body(data_vec.clone())
                .send()
                .await
                .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

            if res.status().is_success() || res.status() == StatusCode::CREATED {
                break;
            }

            let err_info = parse_drive_response_error(res).await;
            if (err_info.is_rate_limit() || err_info.is_server_error()) && retries < 4 {
                retries += 1;
                let backoff_ms = (1000 * (1 << retries)).min(16000);
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                continue;
            }

            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to upload bytes: {err_info}"
            )));
        }

        Ok(())
    }

    async fn read_bytes(&self, remote_path: &str) -> Result<Vec<u8>, RemoteStorageError> {
        let (dir_part, file_name) = Self::split_path(remote_path);
        let parent_result = match dir_part {
            Some(dir) => self.ensure_directory(dir).await,
            None => self.get_or_create_root_folder().await,
        };

        let file_id = match parent_result {
            Ok(parent_id) => {
                match self.find_file_in_parent(&parent_id, file_name).await? {
                    Some(id) => id,
                    None => {
                        tracing::info!(remote_path, file_name, "File not found in expected parent folder; searching Drive by name fallback");
                        match self.find_file_by_name(file_name, dir_part).await? {
                            Some(id) => id,
                            None => return Err(RemoteStorageError::NotFound(remote_path.to_string())),
                        }
                    }
                }
            }
            Err(e) => {
                tracing::info!(remote_path, file_name, err = %e, "Could not resolve parent directory; searching Drive by name fallback");
                match self.find_file_by_name(file_name, dir_part).await? {
                    Some(id) => id,
                    None => return Err(RemoteStorageError::NotFound(remote_path.to_string())),
                }
            }
        };

        let url = format!("{DRIVE_API_BASE}/files/{file_id}?alt=media&supportsAllDrives=true");
        let res = self
            .send_with_retry("Failed to read file from Drive", |token| {
                let u = url.clone();
                let c = self.client.clone();
                async move { c.get(&u).bearer_auth(&token).send().await }
            })
            .await?;

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
            let url = format!("{DRIVE_API_BASE}/files/{file_id}?supportsAllDrives=true");
            let res = self
                .send_with_retry("Failed to delete file on Drive", |token| {
                    let u = url.clone();
                    let c = self.client.clone();
                    async move { c.delete(&u).bearer_auth(&token).send().await }
                })
                .await;

            match res {
                Ok(_) => {}
                Err(RemoteStorageError::ProviderError(ref msg)) if msg.contains("404") => {}
                Err(e) => return Err(e),
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
        let url = format!("{DRIVE_API_BASE}/files/{dir_id}?supportsAllDrives=true");
        let res = self
            .send_with_retry("Failed to delete directory on Drive", |token| {
                let u = url.clone();
                let c = self.client.clone();
                async move { c.delete(&u).bearer_auth(&token).send().await }
            })
            .await;

        match res {
            Ok(_) => {}
            Err(RemoteStorageError::ProviderError(ref msg)) if msg.contains("404") => {}
            Err(e) => return Err(e),
        }

        self.remove_cached_folder(normalized).await;
        Ok(())
    }

    async fn clear_all_data(&self) -> Result<(), RemoteStorageError> {
        let root_id = self.get_or_create_root_folder().await?;
        let q = format!(
            "'{}' in parents and trashed = false",
            escape_drive_query_string(&root_id)
        );

        let mut page_token: Option<String> = None;
        loop {
            let mut url = format!(
                "{DRIVE_API_BASE}/files?supportsAllDrives=true&includeItemsFromAllDrives=true&pageSize=1000&q={}&fields=nextPageToken,files(id,name)",
                urlencoding(&q)
            );
            if let Some(ref pt) = page_token {
                url.push_str("&pageToken=");
                url.push_str(&urlencoding(pt));
            }

            let res = self
                .send_with_retry("List files to clear", |token| {
                    let u = url.clone();
                    let c = self.client.clone();
                    async move { c.get(&u).bearer_auth(&token).send().await }
                })
                .await?;

            let list: DriveFileList = res
                .json()
                .await
                .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

            for file in list.files {
                let del_url = format!("{DRIVE_API_BASE}/files/{}?supportsAllDrives=true", file.id);
                let _ = self
                    .send_with_retry(&format!("Delete '{}'", file.name), |token| {
                        let u = del_url.clone();
                        let c = self.client.clone();
                        async move { c.delete(&u).bearer_auth(&token).send().await }
                    })
                    .await;
            }

            if let Some(next) = list.next_page_token {
                if !next.is_empty() {
                    page_token = Some(next);
                    continue;
                }
            }
            break;
        }

        // Clear local in-memory and shared caches
        {
            let mut cache = self.folder_id_cache.write().await;
            cache.clear();
        }
        {
            let prefix = format!("{}:{}:", self.config.client_id, self.root_folder_name);
            let mut shared = SHARED_FOLDER_CACHE.write().await;
            shared.retain(|k, _| !k.starts_with(&prefix));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_drive_error_user_rate_limit() {
        let body = r#"{
            "error": {
                "code": 403,
                "message": "User Rate Limit Exceeded. Rate of requests for user...",
                "errors": [
                    {
                        "message": "User Rate Limit Exceeded",
                        "domain": "usageLimits",
                        "reason": "userRateLimitExceeded"
                    }
                ]
            }
        }"#;
        let info = parse_drive_error_body(StatusCode::FORBIDDEN, body);
        assert_eq!(info.status, StatusCode::FORBIDDEN);
        assert_eq!(info.reason.as_deref(), Some("userRateLimitExceeded"));
        assert!(info.is_rate_limit());
        assert!(!info.is_auth_error());
        let displayed = info.to_string();
        assert!(displayed.contains("403"));
        assert!(displayed.contains("userRateLimitExceeded"));
        assert!(displayed.contains("User Rate Limit Exceeded"));
    }

    #[test]
    fn test_parse_drive_error_auth() {
        let body = r#"{
            "error": {
                "code": 401,
                "message": "Invalid Credentials",
                "errors": [
                    {
                        "message": "Invalid Credentials",
                        "domain": "global",
                        "reason": "authError"
                    }
                ]
            }
        }"#;
        let info = parse_drive_error_body(StatusCode::UNAUTHORIZED, body);
        assert_eq!(info.status, StatusCode::UNAUTHORIZED);
        assert_eq!(info.reason.as_deref(), Some("authError"));
        assert!(info.is_auth_error());
        assert!(!info.is_rate_limit());
    }

    #[test]
    fn test_parse_drive_error_plain_text_fallback() {
        let body = "<html>403 Forbidden</html>";
        let info = parse_drive_error_body(StatusCode::FORBIDDEN, body);
        assert_eq!(info.status, StatusCode::FORBIDDEN);
        assert_eq!(info.reason, None);
        assert_eq!(info.message, "<html>403 Forbidden</html>");
        assert_eq!(
            info.to_string(),
            "Google Drive API error (403 Forbidden): <html>403 Forbidden</html>"
        );
    }
}
