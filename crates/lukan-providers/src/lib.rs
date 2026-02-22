#![allow(dead_code)]

pub mod anthropic;
pub mod codex_auth;
pub mod contracts;
pub mod fireworks;
pub mod github_copilot;
pub mod nebius;
pub mod openai_codex;
pub mod openai_compat;
pub mod schema_adapter;
pub mod sse;
pub mod think_tag_parser;

use anyhow::{Result, bail};
use lukan_core::config::{CredentialsManager, ProviderName, ResolvedConfig};

pub use contracts::{Provider, StreamParams, SystemPrompt};

/// Factory: create the appropriate provider from resolved config
pub fn create_provider(config: &ResolvedConfig) -> Result<Box<dyn Provider>> {
    let model = config.effective_model();
    let max_tokens = config.config.max_tokens;

    match &config.config.provider {
        ProviderName::Anthropic => {
            let api_key =
                CredentialsManager::get_api_key(&config.credentials, &ProviderName::Anthropic)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                        "Missing Anthropic API key. Set it via `lukan setup` or ANTHROPIC_API_KEY env var"
                    )
                    })?;
            Ok(Box::new(anthropic::AnthropicProvider::new(
                api_key, model, max_tokens,
            )))
        }
        ProviderName::OpenaiCodex => Ok(Box::new(openai_codex::OpenAICodexProvider::new(
            model,
            max_tokens,
            config.credentials.clone(),
        )?)),
        ProviderName::Nebius => {
            let api_key =
                CredentialsManager::get_api_key(&config.credentials, &ProviderName::Nebius)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                        "Missing Nebius API key. Set it via `lukan setup` or NEBIUS_API_KEY env var"
                    )
                    })?;
            Ok(Box::new(nebius::NebiusProvider::new(
                api_key, model, max_tokens,
            )))
        }
        ProviderName::Fireworks => {
            let api_key =
                CredentialsManager::get_api_key(&config.credentials, &ProviderName::Fireworks)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                        "Missing Fireworks API key. Set it via `lukan setup` or FIREWORKS_API_KEY env var"
                    )
                    })?;
            Ok(Box::new(fireworks::FireworksProvider::new(
                api_key, model, max_tokens,
            )))
        }
        ProviderName::GithubCopilot => {
            let token =
                CredentialsManager::get_api_key(&config.credentials, &ProviderName::GithubCopilot)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "Missing GitHub Copilot token. Run: lukan copilot-auth"
                        )
                    })?;
            Ok(Box::new(github_copilot::GitHubCopilotProvider::new(
                token, model, max_tokens,
            )))
        }
        ProviderName::OpenaiCompatible => {
            let base_url = config
                .config
                .openai_compatible_base_url
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Missing OpenAI-compatible base URL. Set it via: lukan config set openaiCompatibleBaseURL http://localhost:8080/v1"
                    )
                })?
                .trim()
                .to_string();

            let api_key = CredentialsManager::get_api_key(
                &config.credentials,
                &ProviderName::OpenaiCompatible,
            )
            .unwrap_or_default();

            let compat_config = openai_compat::OpenAiCompatConfig {
                base_url,
                api_key,
                model,
                max_tokens,
                extra_headers: std::collections::HashMap::new(),
                use_think_tags: true,
                strip_schema: true,
            };

            // Wrap in a simple struct that implements Provider
            Ok(Box::new(OpenAiCompatibleProvider {
                base: openai_compat::OpenAiCompatBase::new(compat_config),
            }))
        }
        provider => {
            bail!(
                "Provider '{}' is not yet implemented. Available: anthropic, openai-codex, nebius, fireworks, github-copilot, openai-compatible",
                provider
            );
        }
    }
}

/// Generic OpenAI-compatible provider for custom endpoints (vLLM, Ollama, LM Studio, etc.)
struct OpenAiCompatibleProvider {
    base: openai_compat::OpenAiCompatBase,
}

#[async_trait::async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        "openai-compatible"
    }

    async fn stream(
        &self,
        params: StreamParams,
        tx: tokio::sync::mpsc::Sender<lukan_core::models::events::StreamEvent>,
    ) -> anyhow::Result<()> {
        self.base.stream(params, tx).await
    }
}
