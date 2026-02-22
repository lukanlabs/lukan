//! GitHub OAuth Device Flow for Copilot authentication.
//!
//! Follows the standard GitHub device flow:
//! <https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow>

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::Deserialize;
use tracing::debug;

const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

// ── Types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default = "default_expires_in")]
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_expires_in() -> u64 {
    900 // 15 minutes
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
struct AccessTokenResponse {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

// ── Device Flow ───────────────────────────────────────────────────────────

/// Authenticate with GitHub using the OAuth Device Flow.
///
/// Displays a user code and verification URL. Polls until the user authorizes
/// the app or a timeout is reached. Returns the access token.
pub async fn auth_copilot_device_flow(client: &Client, client_id: &str) -> Result<String> {
    // Step 1: Request device + user codes
    let resp = client
        .post(DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .form(&[("client_id", client_id), ("scope", "read:user")])
        .send()
        .await
        .context("Failed to request device code from GitHub")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Device code request failed ({status}): {body}");
    }

    let device: DeviceCodeResponse = resp
        .json()
        .await
        .context("Failed to parse device code response")?;

    // Step 2: Display instructions
    println!("\n\x1b[1m\x1b[33m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m");
    println!("\x1b[1mPlease authorize lukan:\x1b[0m\n");
    println!("  1. Visit: \x1b[36m{}\x1b[0m", device.verification_uri);
    println!("  2. Enter code: \x1b[32m\x1b[1m{}\x1b[0m\n", device.user_code);
    println!("\x1b[2mWaiting for authorization...\x1b[0m");
    println!("\x1b[1m\x1b[33m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\x1b[0m\n");

    // Step 3: Poll for access token
    let mut interval = std::time::Duration::from_secs(device.interval);
    let timeout = std::time::Duration::from_secs(device.expires_in.max(900));
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            bail!("Authentication timed out. Please try again.");
        }

        tokio::time::sleep(interval).await;

        let resp = client
            .post(ACCESS_TOKEN_URL)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", client_id),
                ("device_code", &*device.device_code),
                (
                    "grant_type",
                    "urn:ietf:params:oauth:grant-type:device_code",
                ),
            ])
            .send()
            .await;

        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                debug!("Poll request error: {e}");
                continue;
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Token poll failed ({status}): {body}");
        }

        let data: AccessTokenResponse = resp
            .json()
            .await
            .context("Failed to parse token poll response")?;

        if let Some(token) = data.access_token {
            return Ok(token);
        }

        match data.error.as_deref() {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                interval += std::time::Duration::from_secs(5);
                continue;
            }
            Some(err) => {
                let desc = data.error_description.as_deref().unwrap_or("");
                bail!("OAuth error: {err} - {desc}");
            }
            None => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_device_code_response() {
        let json = r#"{
            "device_code": "dc_123",
            "user_code": "ABCD-1234",
            "verification_uri": "https://github.com/login/device",
            "expires_in": 900,
            "interval": 5
        }"#;
        let resp: DeviceCodeResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.device_code, "dc_123");
        assert_eq!(resp.user_code, "ABCD-1234");
        assert_eq!(resp.verification_uri, "https://github.com/login/device");
        assert_eq!(resp.expires_in, 900);
        assert_eq!(resp.interval, 5);
    }

    #[test]
    fn test_deserialize_access_token_success() {
        let json = r#"{"access_token": "ghu_abc123"}"#;
        let resp: AccessTokenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.access_token.as_deref(), Some("ghu_abc123"));
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_deserialize_access_token_pending() {
        let json = r#"{"error": "authorization_pending", "error_description": "waiting"}"#;
        let resp: AccessTokenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.access_token.is_none());
        assert_eq!(resp.error.as_deref(), Some("authorization_pending"));
    }
}
