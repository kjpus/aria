use std::path::Path;
use async_trait::async_trait;
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

use crate::error::RemoteStorageError;
use crate::traits::RemoteStorageBackend;
use crate::types::{RemoteFileRef, StorageStatus, StorageUsage, TransferProgress};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebDavConfig {
    pub url: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WebDavBackend {
    config: WebDavConfig,
    base_url: String,
    client: Client,
}

impl WebDavBackend {
    pub fn new(config: WebDavConfig) -> Self {
        let mut base_url = config.url.trim().to_string();
        if !base_url.ends_with('/') {
            base_url.push('/');
        }
        Self {
            config,
            base_url,
            client: Client::builder().build().unwrap_or_default(),
        }
    }

    fn resolve_url(&self, relative_path: &str) -> Result<String, RemoteStorageError> {
        let trimmed = relative_path.trim();
        if trimmed.is_empty() {
            return Ok(self.base_url.clone());
        }

        // Validate that relative_path contains no parent traversal components
        let rel_path = Path::new(trimmed.trim_start_matches(['/', '\\']));
        for comp in rel_path.components() {
            match comp {
                std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_) => {
                    return Err(RemoteStorageError::ProviderError(format!(
                        "Path traversal attempt detected in WebDAV path: '{relative_path}'"
                    )));
                }
                _ => {}
            }
        }

        let clean_path = trimmed.trim_start_matches(['/', '\\']);
        Ok(format!("{}{}", self.base_url, clean_path))
    }

    fn auth_request(&self, builder: RequestBuilder) -> RequestBuilder {
        match (&self.config.username, &self.config.password) {
            (Some(user), Some(pass)) => builder.basic_auth(user, Some(pass)),
            (Some(user), None) => builder.basic_auth(user, None::<&str>),
            _ => builder,
        }
    }

    fn parse_quota_from_xml(xml: &str) -> StorageUsage {
        let used_bytes = extract_tag_value(xml, "quota-used-bytes")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let free_bytes = extract_tag_value(xml, "quota-available-bytes")
            .and_then(|s| s.parse::<u64>().ok());
        let total_bytes = free_bytes.map(|free| free.saturating_add(used_bytes));

        StorageUsage {
            used_bytes,
            total_bytes,
            free_bytes,
        }
    }
}

fn extract_tag_value<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open_tag_needle = format!("<D:{}", tag);
    let open_tag_needle2 = format!("<d:{}", tag);
    let open_tag_needle3 = format!("<{}", tag);

    let start_idx = xml
        .find(&open_tag_needle)
        .or_else(|| xml.find(&open_tag_needle2))
        .or_else(|| xml.find(&open_tag_needle3))?;

    let close_bracket = xml[start_idx..].find('>')? + start_idx + 1;
    let end_bracket = xml[close_bracket..].find('<')? + close_bracket;

    let value = xml[close_bracket..end_bracket].trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[async_trait]
impl RemoteStorageBackend for WebDavBackend {
    fn provider_id(&self) -> &'static str {
        "webdav"
    }

    async fn test_connection(&self) -> Result<StorageStatus, RemoteStorageError> {
        let propfind_body = r#"<?xml version="1.0" encoding="utf-8" ?>
<D:propfind xmlns:D="DAV:">
  <D:prop>
    <D:resourcetype/>
    <D:quota-available-bytes/>
    <D:quota-used-bytes/>
  </D:prop>
</D:propfind>"#;

        let req = self
            .client
            .request(
                Method::from_bytes(b"PROPFIND").unwrap_or(Method::GET),
                &self.base_url,
            )
            .header("Depth", "0")
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(propfind_body);

        let res = self
            .auth_request(req)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        let status = res.status();
        if status.is_success() || status.as_u16() == 207 {
            let body = res.text().await.unwrap_or_default();
            let usage = Self::parse_quota_from_xml(&body);

            Ok(StorageStatus {
                is_connected: true,
                message: Some("Connected to WebDAV server".into()),
                storage_usage: Some(usage),
            })
        } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            Ok(StorageStatus {
                is_connected: false,
                message: Some(format!("Authentication failed: HTTP {status}")),
                storage_usage: None,
            })
        } else {
            Ok(StorageStatus {
                is_connected: false,
                message: Some(format!("WebDAV returned HTTP {status}")),
                storage_usage: None,
            })
        }
    }

    async fn get_storage_usage(&self) -> Result<StorageUsage, RemoteStorageError> {
        let status = self.test_connection().await?;
        status
            .storage_usage
            .ok_or_else(|| RemoteStorageError::ProviderError("No storage quota reported".into()))
    }

    async fn ensure_directory(&self, path: &str) -> Result<String, RemoteStorageError> {
        let normalized = path.trim_matches(['/', '\\']);
        if normalized.is_empty() {
            return Ok(self.base_url.clone());
        }

        let segments: Vec<&str> = normalized.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
        let mut current_path = String::new();

        for segment in segments {
            if !current_path.is_empty() {
                current_path.push('/');
            }
            current_path.push_str(segment);

            let dir_url = format!("{}/", self.resolve_url(&current_path)?);
            let req = self
                .client
                .request(Method::from_bytes(b"MKCOL").unwrap_or(Method::POST), &dir_url);

            let res = self
                .auth_request(req)
                .send()
                .await
                .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

            let status = res.status();
            // 201 Created = success; 405 Method Not Allowed = folder already exists; 301 = redirect
            if !status.is_success()
                && status != StatusCode::METHOD_NOT_ALLOWED
                && status != StatusCode::MOVED_PERMANENTLY
            {
                // Verify if it already exists with a PROPFIND
                let check = self
                    .auth_request(
                        self.client
                            .request(Method::from_bytes(b"PROPFIND").unwrap_or(Method::GET), &dir_url)
                            .header("Depth", "0"),
                    )
                    .send()
                    .await;

                if let Ok(check_res) = check {
                    if check_res.status().is_success() || check_res.status().as_u16() == 207 {
                        continue;
                    }
                }

                return Err(RemoteStorageError::ProviderError(format!(
                    "Failed to create directory '{segment}': HTTP {status}"
                )));
            }
        }

        self.resolve_url(normalized)
    }

    async fn upload_file(
        &self,
        local_path: &Path,
        remote_path: &str,
        progress: Option<mpsc::Sender<TransferProgress>>,
    ) -> Result<RemoteFileRef, RemoteStorageError> {
        if let Some(parent) = remote_path.rfind(['/', '\\']) {
            self.ensure_directory(&remote_path[..parent]).await?;
        }

        let mut file = File::open(local_path)
            .await
            .map_err(RemoteStorageError::Io)?;
        let file_len = file
            .metadata()
            .await
            .map_err(RemoteStorageError::Io)?
            .len();

        let mut data = Vec::with_capacity(file_len as usize);
        file.read_to_end(&mut data)
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

        let target_url = self.resolve_url(remote_path)?;
        let req = self
            .client
            .put(&target_url)
            .header("Content-Length", file_len.to_string())
            .header("Content-Type", "application/octet-stream")
            .body(data);

        let res = self
            .auth_request(req)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        let status = res.status();
        if !status.is_success() && status != StatusCode::CREATED && status != StatusCode::NO_CONTENT {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to upload file to WebDAV: HTTP {status}"
            )));
        }

        if let Some(ref p) = progress {
            let _ = p
                .send(TransferProgress {
                    bytes_transferred: file_len,
                    total_bytes: file_len,
                })
                .await;
        }

        Ok(RemoteFileRef {
            remote_id: remote_path.to_string(),
            remote_path: target_url,
            size_bytes: file_len,
        })
    }

    async fn write_bytes(&self, remote_path: &str, data: &[u8]) -> Result<(), RemoteStorageError> {
        if let Some(parent) = remote_path.rfind(['/', '\\']) {
            self.ensure_directory(&remote_path[..parent]).await?;
        }

        let target_url = self.resolve_url(remote_path)?;
        let req = self
            .client
            .put(&target_url)
            .header("Content-Length", data.len().to_string())
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec());

        let res = self
            .auth_request(req)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        let status = res.status();
        if !status.is_success() && status != StatusCode::CREATED && status != StatusCode::NO_CONTENT {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to write bytes to WebDAV: HTTP {status}"
            )));
        }

        Ok(())
    }

    async fn read_bytes(&self, remote_path: &str) -> Result<Vec<u8>, RemoteStorageError> {
        let target_url = self.resolve_url(remote_path)?;
        let req = self.client.get(&target_url);

        let res = self
            .auth_request(req)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        if res.status() == StatusCode::NOT_FOUND {
            return Err(RemoteStorageError::NotFound(remote_path.to_string()));
        }

        if !res.status().is_success() {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to read from WebDAV: HTTP {}",
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
        let target_url = self.resolve_url(remote_path)?;
        let req = self.client.delete(&target_url);

        let res = self
            .auth_request(req)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        let status = res.status();
        if !status.is_success() && status != StatusCode::NOT_FOUND {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to delete file on WebDAV: HTTP {status}"
            )));
        }

        Ok(())
    }

    async fn delete_directory(&self, remote_path: &str) -> Result<(), RemoteStorageError> {
        let normalized = remote_path.trim_matches(['/', '\\']);
        if normalized.is_empty() {
            return Ok(());
        }

        let target_url = format!("{}/", self.resolve_url(normalized)?);
        let req = self.client.delete(&target_url);

        let res = self
            .auth_request(req)
            .send()
            .await
            .map_err(|e| RemoteStorageError::ConnectionFailed(e.to_string()))?;

        let status = res.status();
        if !status.is_success() && status != StatusCode::NOT_FOUND {
            return Err(RemoteStorageError::ProviderError(format!(
                "Failed to delete directory on WebDAV: HTTP {status}"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webdav_url_resolution() {
        let config = WebDavConfig {
            url: "https://nas.local:5006/music".to_string(),
            username: Some("user".to_string()),
            password: Some("secret".to_string()),
        };
        let backend = WebDavBackend::new(config);
        assert_eq!(backend.base_url, "https://nas.local:5006/music/");

        let resolved = backend.resolve_url("Beethoven/Sym9/manifest.json").expect("resolve url");
        assert_eq!(
            resolved,
            "https://nas.local:5006/music/Beethoven/Sym9/manifest.json"
        );

        // Path traversal should be rejected
        assert!(backend.resolve_url("../secret.txt").is_err());
        assert!(backend.resolve_url("Beethoven/../../evil.txt").is_err());
    }

    #[test]
    fn test_parse_quota_xml() {
        let xml = r#"<?xml version="1.0" encoding="utf-8" ?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/music/</D:href>
    <D:propstat>
      <D:prop>
        <D:quota-available-bytes>50000000000</D:quota-available-bytes>
        <D:quota-used-bytes>15000000000</D:quota-used-bytes>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

        let usage = WebDavBackend::parse_quota_from_xml(xml);
        assert_eq!(usage.used_bytes, 15_000_000_000);
        assert_eq!(usage.free_bytes, Some(50_000_000_000));
        assert_eq!(usage.total_bytes, Some(65_000_000_000));
    }
}
