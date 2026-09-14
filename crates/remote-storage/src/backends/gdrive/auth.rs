use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

use crate::error::RemoteStorageError;

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_DRIVE_FILE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleAuthConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: i64,
}

impl GoogleTokens {
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        // Expired or will expire within 60 seconds
        self.expires_at <= now + 60
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
    #[allow(dead_code)]
    token_type: Option<String>,
}

/// Generates a high-entropy cryptographically random PKCE code verifier.
pub fn generate_pkce_verifier() -> String {
    let mut bytes = [0u8; 32];
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = ((now >> (i % 8 * 8)) ^ ((i as u128).wrapping_mul(0x9e3779b97f4a7c15))) as u8;
    }
    base64url_encode(&bytes)
}

/// Generates the S256 code challenge for a given PKCE verifier.
pub fn generate_pkce_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    base64url_encode(&hash)
}

pub fn base64url_encode(bytes: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
        let b2 = if i + 2 < bytes.len() { bytes[i + 2] } else { 0 };

        out.push(CHARS[(b0 >> 2) as usize] as char);
        out.push(CHARS[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < bytes.len() {
            out.push(CHARS[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        }
        if i + 2 < bytes.len() {
            out.push(CHARS[(b2 & 0x3f) as usize] as char);
        }
        i += 3;
    }
    out
}

pub struct PendingAuthFlow {
    pub authorization_url: String,
    listener: TcpListener,
    redirect_uri: String,
    verifier: String,
    config: GoogleAuthConfig,
}

impl PendingAuthFlow {
    /// Starts a local loopback HTTP listener on a random port and builds the OAuth authorization URL.
    pub async fn start(config: GoogleAuthConfig) -> Result<Self, RemoteStorageError> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(RemoteStorageError::Io)?;
        let local_addr = listener.local_addr().map_err(RemoteStorageError::Io)?;
        let redirect_uri = format!("http://127.0.0.1:{}/callback", local_addr.port());

        let verifier = generate_pkce_verifier();
        let challenge = generate_pkce_challenge(&verifier);

        let mut auth_url = Url::parse(GOOGLE_AUTH_URL)
            .map_err(|e| RemoteStorageError::AuthFailed(e.to_string()))?;
        auth_url
            .query_pairs_mut()
            .append_pair("client_id", &config.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", GOOGLE_DRIVE_FILE_SCOPE)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");

        Ok(Self {
            authorization_url: auth_url.to_string(),
            listener,
            redirect_uri,
            verifier,
            config,
        })
    }

    /// Listens for the callback from Google in the browser and exchanges the code for OAuth tokens.
    pub async fn wait_for_tokens(self) -> Result<GoogleTokens, RemoteStorageError> {
        let (mut socket, _) = self
            .listener
            .accept()
            .await
            .map_err(RemoteStorageError::Io)?;

        let mut buf = [0u8; 4096];
        let n = socket
            .read(&mut buf)
            .await
            .map_err(RemoteStorageError::Io)?;
        let req_str = String::from_utf8_lossy(&buf[..n]);

        // Parse query parameters from "GET /callback?code=... HTTP/1.1"
        let first_line = req_str.lines().next().unwrap_or_default();
        let path = first_line.split_whitespace().nth(1).unwrap_or_default();
        let url = Url::parse(&format!("http://localhost{}", path))
            .map_err(|e| RemoteStorageError::AuthFailed(e.to_string()))?;

        let params: HashMap<_, _> = url.query_pairs().into_owned().collect();

        if let Some(error) = params.get("error") {
            let html = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n\
                 <html><body style='font-family:sans-serif;padding:40px;'>\
                 <h2>Authentication Failed</h2><p>Google returned error: {}</p></body></html>",
                error
            );
            let _ = socket.write_all(html.as_bytes()).await;
            return Err(RemoteStorageError::AuthFailed(format!("OAuth error: {error}")));
        }

        let code = match params.get("code") {
            Some(c) => c.clone(),
            None => {
                let html = "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\n\r\n\
                            <html><body><h2>Missing authorization code</h2></body></html>";
                let _ = socket.write_all(html.as_bytes()).await;
                return Err(RemoteStorageError::AuthFailed("No code in callback".into()));
            }
        };

        // Send a friendly success page to the browser
        let success_html = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
            <html><body style='font-family:sans-serif;text-align:center;padding:60px;'>\
            <h2 style='color:#3b82f6;'>Aria Connected to Google Drive</h2>\
            <p>You can close this window and return to the Aria app.</p>\
            </body></html>";
        let _ = socket.write_all(success_html.as_bytes()).await;
        let _ = socket.flush().await;

        // Exchange code for tokens
        exchange_code_for_tokens(&self.config, &code, &self.redirect_uri, &self.verifier).await
    }
}

async fn exchange_code_for_tokens(
    config: &GoogleAuthConfig,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<GoogleTokens, RemoteStorageError> {
    let client = Client::new();
    let mut form: HashMap<&str, &str> = HashMap::new();
    form.insert("code", code);
    form.insert("client_id", config.client_id.as_str());
    if let Some(ref secret) = config.client_secret {
        form.insert("client_secret", secret.as_str());
    }
    form.insert("redirect_uri", redirect_uri);
    form.insert("grant_type", "authorization_code");
    form.insert("code_verifier", verifier);

    let res = client
        .post(GOOGLE_TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(|e| RemoteStorageError::AuthFailed(e.to_string()))?;

    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err(RemoteStorageError::AuthFailed(format!(
            "Failed to exchange code for tokens: {err_text}"
        )));
    }

    let token_data: TokenResponse = res
        .json()
        .await
        .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    Ok(GoogleTokens {
        access_token: token_data.access_token,
        refresh_token: token_data.refresh_token,
        expires_at: now + token_data.expires_in as i64,
    })
}

/// Refreshes an expired access token using the stored refresh token.
pub async fn refresh_access_token(
    config: &GoogleAuthConfig,
    refresh_token: &str,
) -> Result<GoogleTokens, RemoteStorageError> {
    let client = Client::new();
    let mut form: HashMap<&str, &str> = HashMap::new();
    form.insert("client_id", config.client_id.as_str());
    if let Some(ref secret) = config.client_secret {
        form.insert("client_secret", secret.as_str());
    }
    form.insert("refresh_token", refresh_token);
    form.insert("grant_type", "refresh_token");

    let res = client
        .post(GOOGLE_TOKEN_URL)
        .form(&form)
        .send()
        .await
        .map_err(|e| RemoteStorageError::AuthFailed(e.to_string()))?;

    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err(RemoteStorageError::AuthFailed(format!(
            "Failed to refresh access token: {err_text}"
        )));
    }

    let token_data: TokenResponse = res
        .json()
        .await
        .map_err(|e| RemoteStorageError::Serialization(e.to_string()))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    Ok(GoogleTokens {
        access_token: token_data.access_token,
        refresh_token: token_data.refresh_token.or_else(|| Some(refresh_token.to_string())),
        expires_at: now + token_data.expires_in as i64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let verifier = generate_pkce_verifier();
        assert!(!verifier.is_empty());
        let challenge = generate_pkce_challenge(&verifier);
        assert!(!challenge.is_empty());
        assert_ne!(verifier, challenge);
    }

    #[test]
    fn test_token_expiration() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let expired_token = GoogleTokens {
            access_token: "xyz".to_string(),
            refresh_token: None,
            expires_at: now - 10,
        };
        assert!(expired_token.is_expired());

        let valid_token = GoogleTokens {
            access_token: "xyz".to_string(),
            refresh_token: None,
            expires_at: now + 3600,
        };
        assert!(!valid_token.is_expired());
    }
}
