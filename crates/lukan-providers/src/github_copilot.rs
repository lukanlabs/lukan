//! GitHub Copilot provider (OpenAI-compatible at api.githubcopilot.com).

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::mpsc;

use lukan_core::models::events::StreamEvent;

use crate::contracts::{Provider, StreamParams};
use crate::openai_compat::{OpenAiCompatBase, OpenAiCompatConfig};

const COPILOT_BASE_URL: &str = "https://api.githubcopilot.com";

pub struct GitHubCopilotProvider {
    base: OpenAiCompatBase,
}

impl GitHubCopilotProvider {
    pub fn new(copilot_token: String, model: String, max_tokens: u32) -> Self {
        let mut extra_headers = HashMap::new();
        extra_headers.insert("Editor-Version".to_string(), "lukan/0.1.0".to_string());
        extra_headers.insert(
            "Editor-Plugin-Version".to_string(),
            "lukan/0.1.0".to_string(),
        );
        extra_headers.insert(
            "OpenAI-Intent".to_string(),
            "conversation-panel".to_string(),
        );

        let config = OpenAiCompatConfig {
            base_url: COPILOT_BASE_URL.to_string(),
            api_key: copilot_token,
            model,
            max_tokens,
            extra_headers,
            use_think_tags: false,
            strip_schema: true,
        };
        Self {
            base: OpenAiCompatBase::new(config),
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
        self.base.stream(params, tx).await
    }
}

// ── Model listing ──────────────────────────────────────────────────────────

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;

pub async fn fetch_github_copilot_models(copilot_token: &str) -> Result<Vec<String>> {
    let client = Client::new();
    let resp = client
        .get(format!("{COPILOT_BASE_URL}/v1/models"))
        .header("authorization", format!("Bearer {copilot_token}"))
        .header("Editor-Version", "lukan/0.1.0")
        .header("Editor-Plugin-Version", "lukan/0.1.0")
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
