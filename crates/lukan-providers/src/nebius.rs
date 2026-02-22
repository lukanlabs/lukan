//! Nebius AI provider (OpenAI-compatible endpoint at api.tokenfactory.nebius.com).

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::mpsc;

use lukan_core::models::events::StreamEvent;

use crate::contracts::{Provider, StreamParams};
use crate::openai_compat::{OpenAiCompatBase, OpenAiCompatConfig};

const NEBIUS_BASE_URL: &str = "https://api.tokenfactory.nebius.com/v1";

pub struct NebiusProvider {
    base: OpenAiCompatBase,
}

impl NebiusProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32) -> Self {
        let config = OpenAiCompatConfig {
            base_url: NEBIUS_BASE_URL.to_string(),
            api_key,
            model,
            max_tokens,
            extra_headers: HashMap::new(),
            use_think_tags: true, // DeepSeek models use <think> tags
            strip_schema: true,
        };
        Self {
            base: OpenAiCompatBase::new(config),
        }
    }
}

#[async_trait]
impl Provider for NebiusProvider {
    fn name(&self) -> &str {
        "nebius"
    }

    async fn stream(&self, params: StreamParams, tx: mpsc::Sender<StreamEvent>) -> anyhow::Result<()> {
        self.base.stream(params, tx).await
    }
}

// ── Model listing ──────────────────────────────────────────────────────────

use anyhow::Result;
use reqwest::Client;
use serde::Deserialize;

/// Vision-capable model patterns
const VISION_PATTERNS: &[&str] = &["VL", "vision", "gemma-3"];

#[derive(Debug)]
pub struct NebiusModelInfo {
    pub id: String,
    pub supports_image_input: bool,
}

pub async fn fetch_nebius_models(api_key: &str) -> Result<Vec<NebiusModelInfo>> {
    let client = Client::new();
    let resp = client
        .get(format!("{NEBIUS_BASE_URL}/models"))
        .header("authorization", format!("Bearer {api_key}"))
        .header("accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Nebius API error: {status} {body}");
    }

    let data: NebiusModelsResponse = resp.json().await?;
    let models = data.data.unwrap_or_default();

    Ok(models
        .into_iter()
        .map(|m| {
            let id_lower = m.id.to_lowercase();
            let supports_image = VISION_PATTERNS
                .iter()
                .any(|p| id_lower.contains(&p.to_lowercase()));
            NebiusModelInfo {
                id: m.id,
                supports_image_input: supports_image,
            }
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct NebiusModelsResponse {
    data: Option<Vec<NebiusModel>>,
}

#[derive(Debug, Deserialize)]
struct NebiusModel {
    id: String,
}
