//! GitHub Copilot session token manager.
//!
//! Exchanges a `gho_` OAuth token for a short-lived JWT session token
//! via `api.github.com/copilot_internal/v2/token`, then caches and
//! auto-refreshes it before expiry.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::debug;

const TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";

/// Refresh 60 seconds before actual expiry.
const REFRESH_MARGIN_SECS: i64 = 60;

// VS Code identity headers used on the exchange request.
const EDITOR_VERSION: &str = "vscode/1.104.3";
const PLUGIN_VERSION: &str = "copilot-chat/0.26.7";
const USER_AGENT: &str = "GitHubCopilotChat/0.26.7";
const API_VERSION: &str = "2025-04-01";

pub struct CopilotTokenManager {
    gho_token: String,
    client: Client,
    cached: Mutex<Option<CachedToken>>,
}

struct CachedToken {
    token: String,
    expires_at: i64,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
    expires_at: i64,
}

impl CopilotTokenManager {
    pub fn new(gho_token: String) -> Self {
        Self {
            gho_token,
            client: Client::new(),
            cached: Mutex::new(None),
        }
    }

    /// Returns a valid session JWT, exchanging or refreshing as needed.
    pub async fn get_token(&self) -> Result<String> {
        let mut guard = self.cached.lock().await;

        if let Some(ref cached) = *guard {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            if now < cached.expires_at - REFRESH_MARGIN_SECS {
                return Ok(cached.token.clone());
            }
            debug!("Copilot session token expired or near-expiry, refreshing");
        }

        let token_resp = self.exchange().await?;
        let token = token_resp.token.clone();
        *guard = Some(CachedToken {
            token: token_resp.token,
            expires_at: token_resp.expires_at,
        });
        Ok(token)
    }

    async fn exchange(&self) -> Result<TokenResponse> {
        debug!("Exchanging gho_ token for Copilot session JWT");

        let resp = self
            .client
            .get(TOKEN_URL)
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("authorization", format!("token {}", self.gho_token))
            .header("editor-version", EDITOR_VERSION)
            .header("editor-plugin-version", PLUGIN_VERSION)
            .header("user-agent", USER_AGENT)
            .header("x-github-api-version", API_VERSION)
            .header("x-vscode-user-agent-library-version", "electron-fetch")
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .context("Failed to reach GitHub Copilot token endpoint")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            bail!("Copilot token exchange failed ({status}): {body}");
        }

        resp.json::<TokenResponse>()
            .await
            .context("Failed to parse Copilot token exchange response")
    }
}
