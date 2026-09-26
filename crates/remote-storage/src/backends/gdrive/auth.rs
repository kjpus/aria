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
    if let Err(e) = getrandom::fill(&mut bytes) {
        tracing::error!("CSPRNG error generating PKCE verifier: {e}");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        let hash = Sha256::digest(format!("{now}:{pid}:pkce_fallback").as_bytes());
        bytes.copy_from_slice(&hash);
    }
    base64url_encode(&bytes)
}

/// Generates a cryptographically random OAuth state token for CSRF protection.
pub fn generate_oauth_state() -> String {
    let mut bytes = [0u8; 32];
    if let Err(e) = getrandom::fill(&mut bytes) {
        tracing::error!("CSPRNG error generating OAuth state: {e}");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        let hash = Sha256::digest(format!("{now}:{pid}:state_fallback").as_bytes());
        bytes.copy_from_slice(&hash);
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

pub const DEFAULT_GOOGLE_CLIENT_ID: &str =
    "349985468666-tpsqh0ncm5h114o22js1aoor6kog9536.apps.googleusercontent.com";

pub fn resolve_google_client_id(provided: Option<&str>) -> String {
    if let Some(id) = provided {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    std::env::var("ARIA_GOOGLE_CLIENT_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_GOOGLE_CLIENT_ID.to_string())
}

pub fn resolve_google_client_secret(provided: Option<&str>) -> Option<String> {
    if let Some(secret) = provided {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    std::env::var("ARIA_GOOGLE_CLIENT_SECRET")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub struct PendingAuthFlow {
    pub authorization_url: String,
    pub redirect_uri: String,
    config: GoogleAuthConfig,
    status_rx: tokio::sync::watch::Receiver<Option<Result<GoogleTokens, String>>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for PendingAuthFlow {
    fn drop(&mut self) {
        if let Some(ref task) = self.task {
            task.abort();
        }
    }
}

impl PendingAuthFlow {
    pub fn config(&self) -> &GoogleAuthConfig {
        &self.config
    }

    pub fn subscribe_status(&self) -> tokio::sync::watch::Receiver<Option<Result<GoogleTokens, String>>> {
        self.status_rx.clone()
    }

    pub fn current_status(&self) -> Option<Result<GoogleTokens, String>> {
        self.status_rx.borrow().clone()
    }

    /// Starts a local loopback HTTP listener on a random port and builds the OAuth authorization URL.
    /// The listener immediately begins accepting incoming requests in a background task.
    pub async fn start(mut config: GoogleAuthConfig) -> Result<Self, RemoteStorageError> {
        config.client_id = resolve_google_client_id(Some(&config.client_id));
        config.client_secret = resolve_google_client_secret(config.client_secret.as_deref());

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(RemoteStorageError::Io)?;
        let local_addr = listener.local_addr().map_err(RemoteStorageError::Io)?;
        let redirect_uri = format!("http://127.0.0.1:{}/callback", local_addr.port());

        let verifier = generate_pkce_verifier();
        let challenge = generate_pkce_challenge(&verifier);
        let state = generate_oauth_state();

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
            .append_pair("state", &state)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");

        let (status_tx, status_rx) = tokio::sync::watch::channel(None);
        let config_clone = config.clone();
        let redirect_uri_clone = redirect_uri.clone();
        let expected_state = state.clone();

        let task = tokio::spawn(async move {
            let res = listen_for_callback(listener, config_clone, redirect_uri_clone, verifier, expected_state).await;
            let _ = status_tx.send(Some(res.map_err(|e| e.to_string())));
        });

        Ok(Self {
            authorization_url: auth_url.to_string(),
            redirect_uri,
            config,
            status_rx,
            task: Some(task),
        })
    }

    /// Listens for the callback from Google in the browser and exchanges the code for OAuth tokens.
    /// If tokens were already acquired by the background task, this returns immediately.
    pub async fn wait_for_tokens(mut self) -> Result<GoogleTokens, RemoteStorageError> {
        loop {
            if let Some(ref res) = *self.status_rx.borrow() {
                return match res {
                    Ok(tokens) => Ok(tokens.clone()),
                    Err(err) => Err(RemoteStorageError::AuthFailed(err.clone())),
                };
            }
            self.status_rx
                .changed()
                .await
                .map_err(|_| RemoteStorageError::AuthFailed("OAuth authorization flow cancelled".into()))?;
        }
    }
}

async fn listen_for_callback(
    listener: TcpListener,
    config: GoogleAuthConfig,
    redirect_uri: String,
    verifier: String,
    expected_state: String,
) -> Result<GoogleTokens, RemoteStorageError> {
    tokio::time::timeout(tokio::time::Duration::from_secs(300), async {
        loop {
            let (mut socket, _) = listener.accept().await.map_err(RemoteStorageError::Io)?;

            let mut buf = [0u8; 8192];
            let n = match socket.read(&mut buf).await {
                Ok(n) if n > 0 => n,
                _ => continue,
            };

            let req_str = String::from_utf8_lossy(&buf[..n]);
            let first_line = req_str.lines().next().unwrap_or_default();
            let mut parts = first_line.split_whitespace();
            let method = parts.next().unwrap_or_default();
            let path = parts.next().unwrap_or_default();

            if path.starts_with("/favicon.ico") {
                let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = socket.write_all(not_found.as_bytes()).await;
                let _ = socket.flush().await;
                let _ = socket.shutdown().await;
                continue;
            }

            if !path.starts_with("/callback") {
                let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = socket.write_all(not_found.as_bytes()).await;
                let _ = socket.flush().await;
                let _ = socket.shutdown().await;
                continue;
            }

            if method != "GET" {
                let not_allowed = "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = socket.write_all(not_allowed.as_bytes()).await;
                let _ = socket.flush().await;
                let _ = socket.shutdown().await;
                continue;
            }

            let url = Url::parse(&format!("http://127.0.0.1{}", path))
                .map_err(|e| RemoteStorageError::AuthFailed(e.to_string()))?;
            let params: HashMap<_, _> = url.query_pairs().into_owned().collect();

            // Validate state parameter to protect against CSRF attacks (RFC 6749 §10.12)
            let received_state = params.get("state").map(|s| s.as_str());
            if received_state != Some(&expected_state) {
                let body = "<!DOCTYPE html><html><body>Invalid or missing OAuth state parameter (CSRF detected)</body></html>";
                let response = format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.as_bytes().len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
                let _ = socket.shutdown().await;
                tracing::warn!("Ignored OAuth callback request with invalid state parameter (possible CSRF)");
                continue;
            }

            if let Some(error) = params.get("error") {
                let body = format!(
                    "<!DOCTYPE html><html><body style='font-family:-apple-system,BlinkMacSystemFont,sans-serif;padding:40px;background:#18181b;color:#f4f4f5;'>\
                    <h2 style='color:#ef4444;'>Google Authentication Failed</h2>\
                    <p>Error: {}</p>\
                    <p style='color:#a1a1aa;'>You can close this window and try again in Aria.</p>\
                    </body></html>",
                    error
                );
                let response = format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.as_bytes().len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
                let _ = socket.shutdown().await;
                return Err(RemoteStorageError::AuthFailed(format!("Google returned error: {error}")));
            }

            let code = match params.get("code") {
                Some(c) => c.clone(),
                None => {
                    let body = "<!DOCTYPE html><html><body>Missing authorization code</body></html>";
                    let response = format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.as_bytes().len(),
                        body
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                    let _ = socket.shutdown().await;
                    return Err(RemoteStorageError::AuthFailed("No code in callback request".into()));
                }
            };

            let body = r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>Aria - Connected to Google Drive</title>
  <style>
    body {
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
      background: #121214;
      color: #f0f0f0;
      display: flex;
      align-items: center;
      justify-content: center;
      height: 100vh;
      margin: 0;
    }
    .card {
      background: #1a1a1f;
      border: 1px solid #2e2e38;
      padding: 2.5rem;
      border-radius: 12px;
      text-align: center;
      max-width: 440px;
      box-shadow: 0 12px 40px rgba(0, 0, 0, 0.6);
    }
    .badge {
      display: inline-block;
      background: rgba(96, 165, 250, 0.15);
      color: #60a5fa;
      font-weight: 600;
      padding: 0.35rem 0.9rem;
      border-radius: 20px;
      font-size: 0.85rem;
      margin-bottom: 1.2rem;
      letter-spacing: 0.03em;
    }
    h2 {
      color: #ffffff;
      font-size: 1.4rem;
      margin: 0 0 0.8rem 0;
      font-weight: 600;
    }
    p {
      color: #a1a1aa;
      font-size: 0.95rem;
      line-height: 1.55;
      margin: 0 0 1.5rem 0;
    }
    .hint {
      font-size: 0.8rem;
      color: #71717a;
    }
  </style>
</head>
<body>
  <div class="card">
    <div class="badge">Aria Classical Player</div>
    <h2>Successfully Connected</h2>
    <p>Google Drive authorization was granted.<br>You can safely close this browser window and return to Aria.</p>
    <div class="hint">Window will attempt to close automatically...</div>
  </div>
  <script>
    setTimeout(() => {
      window.open('', '_self', '');
      window.close();
    }, 2000);
  </script>
</body>
</html>"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.as_bytes().len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.flush().await;
            let _ = socket.shutdown().await;

            return exchange_code_for_tokens(&config, &code, &redirect_uri, &verifier).await;
        }
    })
    .await
    .map_err(|_| RemoteStorageError::AuthFailed("OAuth authorization timed out after 5 minutes".into()))?
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
        refresh_token: token_data
            .refresh_token
            .or_else(|| Some(refresh_token.to_string())),
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

    #[test]
    fn test_resolve_google_client_id() {
        // Fallback to default when None or empty
        assert_eq!(resolve_google_client_id(None), DEFAULT_GOOGLE_CLIENT_ID);
        assert_eq!(resolve_google_client_id(Some("")), DEFAULT_GOOGLE_CLIENT_ID);
        assert_eq!(
            resolve_google_client_id(Some("   ")),
            DEFAULT_GOOGLE_CLIENT_ID
        );

        // Custom client id preserved
        let custom = "custom-client-id-123.apps.googleusercontent.com";
        assert_eq!(resolve_google_client_id(Some(custom)), custom);
    }

    #[tokio::test]
    async fn test_pending_auth_flow_loopback_handling() {
        let config = GoogleAuthConfig {
            client_id: "test-client-id".to_string(),
            client_secret: None,
        };
        let flow = PendingAuthFlow::start(config).await.expect("start flow");
        let redirect_uri = flow.redirect_uri.clone();
        let mut status_rx = flow.subscribe_status();

        // 1. Sending request to /favicon.ico should return 404 and not break loop
        let client = Client::new();
        let favicon_url = redirect_uri.replace("/callback", "/favicon.ico");
        let res = client.get(&favicon_url).send().await.expect("send favicon");
        assert_eq!(res.status(), reqwest::StatusCode::NOT_FOUND);

        let auth_url_parsed = Url::parse(&flow.authorization_url).expect("parse auth url");
        let state = auth_url_parsed
            .query_pairs()
            .find(|(k, _)| k == "state")
            .expect("state present")
            .1
            .to_string();

        // 2. Sending request with missing or incorrect state should return 400 and trigger CSRF detection
        let bad_state_url = format!("{redirect_uri}?error=access_denied&state=tampered");
        let res = client.get(&bad_state_url).send().await.expect("send bad state");
        assert_eq!(res.status(), reqwest::StatusCode::BAD_REQUEST);

        // 3. Sending error callback with valid state should update status to Google returned error
        let error_url = format!("{redirect_uri}?error=access_denied&state={state}");
        let res = client.get(&error_url).send().await.expect("send error callback");
        assert_eq!(res.status(), reqwest::StatusCode::BAD_REQUEST);

        // Verify status_rx receives error
        status_rx.changed().await.expect("status changed");
        let current = status_rx.borrow().clone();
        assert!(current.is_some());
        let err_result = current.unwrap();
        assert!(err_result.is_err());
        let err_msg = err_result.unwrap_err();
        assert!(err_msg.contains("access_denied"));
    }
}
