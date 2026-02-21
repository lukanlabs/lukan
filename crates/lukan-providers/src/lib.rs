#![allow(dead_code)]

pub mod anthropic;
pub mod contracts;
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
            let api_key = CredentialsManager::get_api_key(&config.credentials, &ProviderName::Anthropic)
                .ok_or_else(|| anyhow::anyhow!(
                    "Missing Anthropic API key. Set it via `lukan setup` or ANTHROPIC_API_KEY env var"
                ))?;
            Ok(Box::new(anthropic::AnthropicProvider::new(
                api_key, model, max_tokens,
            )))
        }
        provider => {
            bail!(
                "Provider '{}' is not yet implemented in the Rust version. Only 'anthropic' is available in Phase 1.",
                provider
            );
        }
    }
}
