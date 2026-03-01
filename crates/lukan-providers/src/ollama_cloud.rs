//! Ollama Cloud provider (OpenAI-compatible endpoint at ollama.com).

use std::collections::HashMap;

use async_trait::async_trait;
use tokio::sync::mpsc;

use lukan_core::models::events::StreamEvent;

use crate::contracts::{Provider, StreamParams};
use crate::openai_compat::{OpenAiCompatBase, OpenAiCompatConfig};

const OLLAMA_CLOUD_BASE_URL: &str = "https://ollama.com/v1";

pub struct OllamaCloudProvider {
    base: OpenAiCompatBase,
}

impl OllamaCloudProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32, supports_images: bool) -> Self {
        let config = OpenAiCompatConfig {
            base_url: OLLAMA_CLOUD_BASE_URL.to_string(),
            api_key,
            model,
            max_tokens,
            extra_headers: HashMap::new(),
            use_think_tags: true,
            strip_schema: true,
            supports_images,
        };
        Self {
            base: OpenAiCompatBase::new(config),
        }
    }
}

#[async_trait]
impl Provider for OllamaCloudProvider {
    fn name(&self) -> &str {
        "ollama-cloud"
    }

    fn supports_images(&self) -> bool {
        self.base.config.supports_images
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

#[derive(Debug)]
pub struct OllamaCloudModel {
    pub name: String,
    pub model: String,
}

pub async fn fetch_ollama_cloud_models(api_key: &str) -> Result<Vec<OllamaCloudModel>> {
    let client = Client::new();
    let resp = client
        .get("https://ollama.com/api/tags")
        .header("authorization", format!("Bearer {api_key}"))
        .header("accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Ollama Cloud API error: {status} {body}");
    }

    let data: OllamaTagsResponse = resp.json().await?;
    let mut models: Vec<OllamaCloudModel> = data
        .models
        .unwrap_or_default()
        .into_iter()
        .map(|m| OllamaCloudModel {
            name: m.name.clone(),
            model: m.model.unwrap_or(m.name),
        })
        .collect();

    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Option<Vec<OllamaTagModel>>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagModel {
    name: String,
    model: Option<String>,
}
