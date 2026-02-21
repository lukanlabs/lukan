//! OAuth PKCE authentication for OpenAI Codex.
//!
//! Supports two flows:
//! - Browser flow: opens browser, listens on localhost:1455 for callback
//! - Device code flow: displays code for headless environments

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::Rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{debug, info, warn};

// ── Constants ─────────────────────────────────────────────────────────────

const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const ISSUER: &str = "https://auth.openai.com";
const AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const DEVICE_USERCODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const REDIRECT_PORT: u16 = 1455;
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const SCOPES: &str = "openid profile email offline_access";
const DEFAULT_EXPIRY_SECS: u64 = 28 * 24 * 3600; // 28 days

// ── Types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Milliseconds since epoch
    pub expires_at: u64,
}

#[derive(Debug, Deserialize)]
struct TokenExchangeResponse {
    #[serde(default)]
    id_token: Option<String>,
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RefreshResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DeviceUserCodeResponse {
    device_auth_id: String,
    #[serde(alias = "usercode")]
    user_code: Option<String>,
    #[serde(
        default = "default_interval",
        deserialize_with = "deserialize_string_or_u64"
    )]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

/// Deserialize a value that may be a number or a string containing a number.
fn deserialize_string_or_u64<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct StringOrU64;

    impl<'de> de::Visitor<'de> for StringOrU64 {
        type Value = u64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a number or a string containing a number")
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<u64, E> {
            Ok(v)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<u64, E> {
            u64::try_from(v).map_err(de::Error::custom)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<u64, E> {
            v.parse().map_err(de::Error::custom)
        }
    }

    deserializer.deserialize_any(StringOrU64)
}

#[derive(Debug, Deserialize)]
struct DeviceTokenResponse {
    authorization_code: String,
    #[serde(default)]
    code_verifier: Option<String>,
    #[serde(default)]
    code_challenge: Option<String>,
}

struct PkcePair {
    verifier: String,
    challenge: String,
}

// ── PKCE Generation ───────────────────────────────────────────────────────

fn generate_pkce() -> PkcePair {
    let random_bytes: [u8; 32] = rand::rng().random();
    let verifier = URL_SAFE_NO_PAD.encode(random_bytes);

    let hash = Sha256::digest(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hash);

    PkcePair {
        verifier,
        challenge,
    }
}

fn generate_state() -> String {
    let random_bytes: [u8; 32] = rand::rng().random();
    URL_SAFE_NO_PAD.encode(random_bytes)
}

// ── Browser Flow ──────────────────────────────────────────────────────────

/// Authenticate via browser: opens auth URL, listens on localhost for callback.
pub async fn auth_browser_flow(client: &Client) -> Result<CodexTokens> {
    let pkce = generate_pkce();
    let state = generate_state();

    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}&id_token_add_organizations=true&codex_cli_simplified_flow=true&originator=codex_cli",
        AUTH_URL,
        CLIENT_ID,
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(SCOPES),
        pkce.challenge,
        state,
    );

    info!("Opening browser for authentication...");
    println!("\nOpening browser for authentication...");
    println!("If the browser doesn't open, visit:\n  {}\n", auth_url);

    if let Err(e) = open::that(&auth_url) {
        warn!("Failed to open browser: {e}");
        println!("Please open the URL above manually.");
    }

    // Start local HTTP server to receive callback
    let listener = TcpListener::bind(format!("127.0.0.1:{REDIRECT_PORT}"))
        .await
        .context("Failed to bind callback server on port 1455")?;

    info!("Listening on port {REDIRECT_PORT} for callback...");

    let code = tokio::time::timeout(std::time::Duration::from_secs(300), async {
        loop {
            let (mut stream, _) = listener.accept().await?;
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await?;
            let request = String::from_utf8_lossy(&buf[..n]);

            // Extract the GET path
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("");

            if !path.starts_with("/auth/callback") {
                let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                stream.write_all(response.as_bytes()).await.ok();
                continue;
            }

            // Parse query params
            let query = path.split('?').nth(1).unwrap_or("");
            let params: HashMap<&str, &str> = query
                .split('&')
                .filter_map(|pair| {
                    let mut parts = pair.splitn(2, '=');
                    Some((parts.next()?, parts.next().unwrap_or("")))
                })
                .collect();

            // Validate state
            if params.get("state") != Some(&state.as_str()) {
                let body = "Authentication failed: state mismatch.";
                let response = format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.ok();
                bail!("State mismatch in callback");
            }

            // Check for error
            if let Some(error) = params.get("error") {
                let desc = params.get("error_description").unwrap_or(error);
                let body = format!("Authentication failed: {desc}");
                let response = format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.ok();
                bail!("Auth error: {desc}");
            }

            // Extract code
            let code = params
                .get("code")
                .context("No 'code' parameter in callback")?
                .to_string();

            let body = "Authentication successful! You can close this tab.";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.ok();

            return Ok::<String, anyhow::Error>(code);
        }
    })
    .await
    .context("Authentication timed out (5 minutes)")??;

    // Exchange code for tokens
    exchange_code(client, &code, REDIRECT_URI, &pkce.verifier).await
}

// ── Device Code Flow ──────────────────────────────────────────────────────

/// Authenticate via device code: displays a code for the user to enter at a URL.
pub async fn auth_device_flow(client: &Client) -> Result<CodexTokens> {
    info!("Starting device code flow...");

    // Request device code
    let resp = client
        .post(DEVICE_USERCODE_URL)
        .json(&serde_json::json!({
            "client_id": CLIENT_ID,
        }))
        .send()
        .await
        .context("Failed to request device code")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Device code request failed ({status}): {body}");
    }

    let device_resp: DeviceUserCodeResponse = resp
        .json()
        .await
        .context("Failed to parse device code response")?;

    let user_code = device_resp
        .user_code
        .context("No user code in device response")?;

    println!("\n  To authenticate, visit:");
    println!("    {ISSUER}/codex/device\n");
    println!("  And enter code: {user_code}\n");
    println!("  Waiting for authorization...");

    // Poll for token
    let poll_interval = std::time::Duration::from_secs(device_resp.interval);
    let timeout = std::time::Duration::from_secs(15 * 60);
    let start = std::time::Instant::now();

    let (auth_code, code_verifier) = loop {
        if start.elapsed() > timeout {
            bail!("Device authorization timed out (15 minutes)");
        }

        tokio::time::sleep(poll_interval).await;

        let resp = client
            .post(DEVICE_TOKEN_URL)
            .json(&serde_json::json!({
                "device_auth_id": device_resp.device_auth_id,
                "user_code": user_code,
            }))
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                debug!("Device poll error: {e}");
                continue;
            }
        };

        let status = resp.status();
        if status == reqwest::StatusCode::FORBIDDEN
            || status == reqwest::StatusCode::NOT_FOUND
            || status == reqwest::StatusCode::BAD_REQUEST
        {
            // Not authorized yet
            continue;
        }

        if status.is_success() {
            let device_token: DeviceTokenResponse = resp
                .json()
                .await
                .context("Failed to parse device token response")?;
            break (device_token.authorization_code, device_token.code_verifier);
        }

        let body = resp.text().await.unwrap_or_default();
        debug!("Device poll unexpected status {status}: {body}");
    };

    println!("  Authorized! Exchanging tokens...\n");

    let verifier = code_verifier.unwrap_or_default();
    exchange_code(client, &auth_code, DEVICE_REDIRECT_URI, &verifier).await
}

// ── Code Exchange ─────────────────────────────────────────────────────────

async fn exchange_code(
    client: &Client,
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<CodexTokens> {
    let params = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("client_id", CLIENT_ID),
        ("code_verifier", code_verifier),
    ];

    let resp = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .context("Token exchange request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Token exchange failed ({status}): {body}");
    }

    let token_resp: TokenExchangeResponse = resp
        .json()
        .await
        .context("Failed to parse token exchange response")?;

    let now_ms = now_millis();
    let expires_at = now_ms + token_resp.expires_in.unwrap_or(DEFAULT_EXPIRY_SECS) * 1000;

    // Try to exchange for API key (optional, best-effort)
    let access_token = if let Some(ref id_token) = token_resp.id_token {
        match try_api_key_exchange(client, id_token).await {
            Ok(api_key) => {
                info!("Successfully exchanged for API key");
                api_key
            }
            Err(e) => {
                debug!("API key exchange failed (using access_token): {e}");
                token_resp.access_token.clone()
            }
        }
    } else {
        token_resp.access_token.clone()
    };

    Ok(CodexTokens {
        access_token,
        refresh_token: token_resp.refresh_token.unwrap_or_default(),
        expires_at,
    })
}

/// Try to exchange id_token for an OpenAI API key (best-effort).
async fn try_api_key_exchange(client: &Client, id_token: &str) -> Result<String> {
    let params = [
        (
            "grant_type",
            "urn:ietf:params:oauth:grant-type:token-exchange",
        ),
        ("client_id", CLIENT_ID),
        ("requested_token", "openai-api-key"),
        ("subject_token", id_token),
        (
            "subject_token_type",
            "urn:ietf:params:oauth:token-type:id_token",
        ),
    ];

    let resp = client.post(TOKEN_URL).form(&params).send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        bail!("API key exchange failed ({status})");
    }

    #[derive(Deserialize)]
    struct ApiKeyResponse {
        access_token: String,
    }

    let api_key_resp: ApiKeyResponse = resp.json().await?;
    Ok(api_key_resp.access_token)
}

// ── Token Refresh ─────────────────────────────────────────────────────────

/// Refresh the access token using the refresh token.
pub async fn refresh_tokens(client: &Client, refresh_token: &str) -> Result<CodexTokens> {
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", CLIENT_ID),
        ("refresh_token", refresh_token),
        ("scope", "openid profile email"),
    ];

    let resp = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await
        .context("Token refresh request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!(
            "Token refresh failed ({status}): {body}\n\
             Run 'lukan codex-auth' to re-authenticate."
        );
    }

    let refresh_resp: RefreshResponse = resp
        .json()
        .await
        .context("Failed to parse refresh response")?;

    let now_ms = now_millis();
    let expires_at = now_ms + refresh_resp.expires_in.unwrap_or(DEFAULT_EXPIRY_SECS) * 1000;

    Ok(CodexTokens {
        access_token: refresh_resp.access_token,
        refresh_token: refresh_resp
            .refresh_token
            .unwrap_or_else(|| refresh_token.to_string()),
        expires_at,
    })
}

/// Check if tokens need refresh (within 5 minutes of expiry).
pub fn needs_refresh(tokens: &CodexTokens) -> bool {
    let five_min_ms = 5 * 60 * 1000;
    now_millis() > tokens.expires_at.saturating_sub(five_min_ms)
}

// ── JWT Helpers ───────────────────────────────────────────────────────────

/// Extract the ChatGPT account ID from a JWT access token (best-effort).
pub fn extract_account_id(access_token: &str) -> Option<String> {
    let parts: Vec<&str> = access_token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;

    claims
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_account_id"))
        .and_then(|id| id.as_str())
        .map(|s| s.to_string())
}

// ── Utility ───────────────────────────────────────────────────────────────

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let pkce = generate_pkce();
        assert!(!pkce.verifier.is_empty());
        assert!(!pkce.challenge.is_empty());
        // Verify challenge is SHA256 of verifier
        let hash = Sha256::digest(pkce.verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(hash);
        assert_eq!(pkce.challenge, expected);
    }

    #[test]
    fn test_state_generation() {
        let state = generate_state();
        assert!(!state.is_empty());
        // Should be base64url encoded 32 bytes
        assert!(state.len() >= 40);
    }

    #[test]
    fn test_needs_refresh() {
        let tokens = CodexTokens {
            access_token: "test".to_string(),
            refresh_token: "test".to_string(),
            expires_at: now_millis() + 60_000, // 1 minute from now
        };
        // Should need refresh (within 5 min window)
        assert!(needs_refresh(&tokens));

        let tokens = CodexTokens {
            access_token: "test".to_string(),
            refresh_token: "test".to_string(),
            expires_at: now_millis() + 10 * 60_000, // 10 minutes from now
        };
        // Should NOT need refresh
        assert!(!needs_refresh(&tokens));
    }

    #[test]
    fn test_extract_account_id_invalid() {
        assert_eq!(extract_account_id("not-a-jwt"), None);
        assert_eq!(extract_account_id("a.b.c"), None);
    }
}
