#![allow(dead_code)]

pub mod anthropic;
pub mod codex_auth;
pub mod contracts;
pub mod copilot_auth;
pub mod copilot_token;
pub mod fireworks;
pub mod github_copilot;
pub mod nebius;
pub mod ollama_cloud;
pub mod openai_codex;
pub mod openai_compat;
pub mod schema_adapter;
pub mod sse;
pub mod think_tag_parser;

use anyhow::{Result, bail};
use lukan_core::config::types::{AppConfig, Credentials};
use lukan_core::config::{CredentialsManager, ProviderName, ResolvedConfig};
use tracing::debug;

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
            let api_key = CredentialsManager::get_api_key(
                &config.credentials,
                &ProviderName::Nebius,
            )
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
            let supports_images = is_vision_model(&model, config);
            Ok(Box::new(fireworks::FireworksProvider::new(
                api_key,
                model,
                max_tokens,
                supports_images,
            )))
        }
        ProviderName::GithubCopilot => {
            let token =
                CredentialsManager::get_api_key(&config.credentials, &ProviderName::GithubCopilot)
                    .ok_or_else(|| {
                        anyhow::anyhow!("Missing GitHub Copilot token. Run: lukan copilot-auth")
                    })?;
            let token_manager = std::sync::Arc::new(copilot_token::CopilotTokenManager::new(token));
            Ok(Box::new(github_copilot::GitHubCopilotProvider::new(
                token_manager,
                model,
                max_tokens,
            )))
        }
        ProviderName::OllamaCloud => {
            let api_key =
                CredentialsManager::get_api_key(&config.credentials, &ProviderName::OllamaCloud)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                        "Missing Ollama Cloud API key. Set it via `lukan setup` or OLLAMA_API_KEY env var"
                    )
                    })?;
            let supports_images = is_vision_model(&model, config);
            Ok(Box::new(ollama_cloud::OllamaCloudProvider::new(
                api_key,
                model,
                max_tokens,
                supports_images,
            )))
        }
        ProviderName::OpenaiCompatible => {
            let raw_base_url = config
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

            let base_url = openai_compat::normalize_base_url(&raw_base_url);

            let api_key = CredentialsManager::get_api_key(
                &config.credentials,
                &ProviderName::OpenaiCompatible,
            )
            .unwrap_or_default();

            let supports_images = is_vision_model(&model, config);
            let compat_config = openai_compat::OpenAiCompatConfig {
                base_url,
                api_key,
                model,
                max_tokens,
                extra_headers: std::collections::HashMap::new(),
                use_think_tags: true,
                strip_schema: true,
                supports_images,
            };

            // Wrap in a simple struct that implements Provider
            Ok(Box::new(OpenAiCompatibleProvider {
                base: openai_compat::OpenAiCompatBase::new(compat_config),
            }))
        }
        provider => {
            bail!(
                "Provider '{}' is not yet implemented. Available: anthropic, openai-codex, nebius, fireworks, github-copilot, ollama-cloud, openai-compatible",
                provider
            );
        }
    }
}

/// Create a vision-capable provider for the image preprocessor.
///
/// `vision_model` is an optional `"provider:model"` string from config.
/// Falls back to `anthropic:claude-haiku-4-5-20251001` if an Anthropic key is available.
/// Returns `None` on any error (non-fatal).
pub fn create_vision_provider(
    vision_model: Option<&str>,
    credentials: &Credentials,
) -> Option<Box<dyn Provider>> {
    let (provider_str, model_str) = match vision_model {
        Some(spec) => {
            // Parse "provider:model"
            let (p, m) = spec.split_once(':')?;
            (p.to_string(), m.to_string())
        }
        None => {
            // Fallback: use Anthropic Haiku if key is available
            if CredentialsManager::get_api_key(credentials, &ProviderName::Anthropic).is_some() {
                (
                    "anthropic".to_string(),
                    "claude-haiku-4-5-20251001".to_string(),
                )
            } else {
                return None;
            }
        }
    };

    let provider_name: ProviderName =
        serde_json::from_value(serde_json::Value::String(provider_str.clone())).ok()?;

    let resolved = ResolvedConfig {
        config: AppConfig {
            provider: provider_name,
            model: Some(model_str),
            max_tokens: 1024,
            ..AppConfig::default()
        },
        credentials: credentials.clone(),
    };

    match create_provider(&resolved) {
        Ok(p) => {
            debug!("Vision provider created: {}", p.name());
            Some(p)
        }
        Err(e) => {
            debug!("Failed to create vision provider ({provider_str}): {e}");
            None
        }
    }
}

/// Check if a model supports image inputs:
/// 1. From `config.vision_models` (set by model picker from API capabilities)
/// 2. Fallback heuristic on model name
fn is_vision_model(model: &str, config: &ResolvedConfig) -> bool {
    if let Some(ref vision_models) = config.config.vision_models
        && vision_models.iter().any(|v| v == model)
    {
        return true;
    }
    // Fallback heuristic for models not in the list
    let lower = model.to_lowercase();
    lower.contains("vision")
        || lower.contains("-vl")
        || lower.contains("multimodal")
        || lower.contains("llava")
        || lower.contains("gemma-3")
        || lower.contains("llama-4")
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

    fn supports_images(&self) -> bool {
        self.base.config.supports_images
    }

    async fn stream(
        &self,
        params: StreamParams,
        tx: tokio::sync::mpsc::Sender<lukan_core::models::events::StreamEvent>,
    ) -> anyhow::Result<()> {
        self.base.stream(params, tx).await
    }
}
