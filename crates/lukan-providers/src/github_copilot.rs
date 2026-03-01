//! GitHub Copilot provider (OpenAI-compatible at api.githubcopilot.com).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use lukan_core::models::events::StreamEvent;

use crate::contracts::{Provider, StreamParams};
use crate::copilot_token::CopilotTokenManager;
use crate::openai_compat::{OpenAiCompatBase, OpenAiCompatConfig};

const COPILOT_BASE_URL: &str = "https://api.githubcopilot.com";

// VS Code identity headers for API requests.
const EDITOR_VERSION: &str = "vscode/1.104.3";
const PLUGIN_VERSION: &str = "copilot-chat/0.26.7";

pub struct GitHubCopilotProvider {
    token_manager: Arc<CopilotTokenManager>,
    model: String,
    max_tokens: u32,
}

impl GitHubCopilotProvider {
    pub fn new(token_manager: Arc<CopilotTokenManager>, model: String, max_tokens: u32) -> Self {
        Self {
            token_manager,
            model,
            max_tokens,
        }
    }

    fn build_config(&self, session_token: String) -> OpenAiCompatConfig {
        let mut extra_headers = HashMap::new();
        extra_headers.insert(
            "copilot-integration-id".to_string(),
            "vscode-chat".to_string(),
        );
        extra_headers.insert("editor-version".to_string(), EDITOR_VERSION.to_string());
        extra_headers.insert(
            "editor-plugin-version".to_string(),
            PLUGIN_VERSION.to_string(),
        );
        extra_headers.insert(
            "openai-intent".to_string(),
            "conversation-panel".to_string(),
        );
        extra_headers.insert("x-request-id".to_string(), uuid::Uuid::new_v4().to_string());

        OpenAiCompatConfig {
            base_url: COPILOT_BASE_URL.to_string(),
            api_key: session_token,
            model: self.model.clone(),
            max_tokens: self.max_tokens,
            extra_headers,
            use_think_tags: false,
            strip_schema: true,
            supports_images: true,
        }
    }
}

#[async_trait]
impl Provider for GitHubCopilotProvider {
    fn name(&self) -> &str {
        "github-copilot"
    }

    fn supports_images(&self) -> bool {
        true
    }

    async fn stream(
        &self,
        params: StreamParams,
        tx: mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<()> {
        let token = self.token_manager.get_token().await?;
        let config = self.build_config(token);
        let base = OpenAiCompatBase::new(config);
        base.stream(params, tx).await
    }
}

// ── Model listing ──────────────────────────────────────────────────────────

use anyhow::Result;
use serde::Deserialize;

pub async fn fetch_github_copilot_models(
    token_manager: &CopilotTokenManager,
) -> Result<Vec<String>> {
    let token = token_manager.get_token().await?;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{COPILOT_BASE_URL}/models"))
        .header("authorization", format!("Bearer {token}"))
        .header("copilot-integration-id", "vscode-chat")
        .header("editor-version", EDITOR_VERSION)
        .header("editor-plugin-version", PLUGIN_VERSION)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("GitHub Copilot API error: {status} {body}");
    }

    let data: CopilotModelsResponse = resp.json().await?;
    Ok(data.data.into_iter().map(|m| m.id).collect())
}

#[derive(Debug, Deserialize)]
struct CopilotModelsResponse {
    data: Vec<CopilotModel>,
}

#[derive(Debug, Deserialize)]
struct CopilotModel {
    id: String,
}
