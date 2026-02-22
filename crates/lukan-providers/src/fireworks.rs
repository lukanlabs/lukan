//! Fireworks AI provider (OpenAI-compatible with session affinity support).

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::mpsc;

use lukan_core::models::events::StreamEvent;

use crate::contracts::{Provider, StreamParams};
use crate::openai_compat::{OpenAiCompatBase, OpenAiCompatConfig};

const FIREWORKS_BASE_URL: &str = "https://api.fireworks.ai/inference/v1";

pub struct FireworksProvider {
    base: OpenAiCompatBase,
}

impl FireworksProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32) -> Self {
        let config = OpenAiCompatConfig {
            base_url: FIREWORKS_BASE_URL.to_string(),
            api_key,
            model,
            max_tokens,
            extra_headers: HashMap::new(),
            use_think_tags: true, // Some models use <think> tags
            strip_schema: true,
        };
        Self {
            base: OpenAiCompatBase::new(config),
        }
    }
}

#[async_trait]
impl Provider for FireworksProvider {
    fn name(&self) -> &str {
        "fireworks"
    }

    async fn stream(&self, params: StreamParams, tx: mpsc::Sender<StreamEvent>) -> anyhow::Result<()> {
        self.base.stream(params, tx).await
    }
}

// ── Model listing ──────────────────────────────────────────────────────────

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;

#[derive(Debug)]
pub struct FireworksModelInfo {
    pub id: String,
    pub display_name: String,
    pub supports_image_input: bool,
    pub context_length: Option<u64>,
}

pub async fn fetch_fireworks_models(api_key: &str) -> Result<Vec<FireworksModelInfo>> {
    let client = Client::new();
    let resp = client
        .get("https://api.fireworks.ai/v1/accounts/fireworks/models?filter=supports_serverless%3Dtrue")
        .header("authorization", format!("Bearer {api_key}"))
        .header("accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Fireworks API error: {status} {body}");
    }

    let data: FireworksModelsResponse = resp.json().await?;
    let models = data.models.unwrap_or_default();

    Ok(models
        .into_iter()
        .filter(|m| m.name.is_some())
        .map(|m| {
            let name = m.name.unwrap();
            let display = m
                .display_name
                .unwrap_or_else(|| name.split('/').next_back().unwrap_or(&name).to_string());
            FireworksModelInfo {
                id: name,
                display_name: display,
                supports_image_input: m.supports_image_input.unwrap_or(false),
                context_length: m.context_length,
            }
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct FireworksModelsResponse {
    models: Option<Vec<FireworksModel>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FireworksModel {
    name: Option<String>,
    display_name: Option<String>,
    supports_image_input: Option<bool>,
    context_length: Option<u64>,
}
