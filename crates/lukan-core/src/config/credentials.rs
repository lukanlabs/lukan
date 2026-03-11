use anyhow::{Context, Result};
use std::os::unix::fs::PermissionsExt;
use tracing::debug;

use super::paths::LukanPaths;
use super::types::Credentials;

/// Manages API credentials with env var fallback
pub struct CredentialsManager;

impl CredentialsManager {
    /// Load credentials from file + environment variables
    ///
    /// Priority: credentials.json > environment variables
    pub async fn load() -> Result<Credentials> {
        let mut creds = Self::load_from_file().await.unwrap_or_default();
        Self::merge_env_vars(&mut creds);
        Ok(creds)
    }

    /// Load credentials from the credentials.json file
    async fn load_from_file() -> Result<Credentials> {
        let path = LukanPaths::credentials_file();

        if !path.exists() {
            debug!("No credentials file found");
            return Ok(Credentials::default());
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .context("Failed to read credentials file")?;

        let creds: Credentials =
            serde_json::from_str(&content).context("Failed to parse credentials.json")?;

        Ok(creds)
    }

    /// Merge environment variables into credentials (file values take priority)
    fn merge_env_vars(creds: &mut Credentials) {
        macro_rules! env_fallback {
            ($field:ident, $var:expr) => {
                if creds.$field.is_none() {
                    if let Ok(val) = std::env::var($var) {
                        if !val.is_empty() {
                            creds.$field = Some(val);
                        }
                    }
                }
            };
        }

        env_fallback!(nebius_api_key, "NEBIUS_API_KEY");
        env_fallback!(anthropic_api_key, "ANTHROPIC_API_KEY");
        env_fallback!(fireworks_api_key, "FIREWORKS_API_KEY");

        env_fallback!(brave_api_key, "BRAVE_API_KEY");
        env_fallback!(tavily_api_key, "TAVILY_API_KEY");
        env_fallback!(openai_api_key, "OPENAI_API_KEY");
        env_fallback!(zai_api_key, "ZAI_API_KEY");
        env_fallback!(ollama_cloud_api_key, "OLLAMA_API_KEY");
        env_fallback!(openai_compatible_api_key, "OPENAI_COMPATIBLE_API_KEY");
        env_fallback!(lukan_cloud_api_key, "LUKAN_CLOUD_API_KEY");
        env_fallback!(gemini_api_key, "GEMINI_API_KEY");
    }

    /// Save credentials to disk with restricted permissions (0o600)
    pub async fn save(creds: &Credentials) -> Result<()> {
        LukanPaths::ensure_dirs().await?;
        let path = LukanPaths::credentials_file();
        let content = serde_json::to_string_pretty(creds)?;
        tokio::fs::write(&path, &content)
            .await
            .context("Failed to write credentials file")?;

        // Set restrictive permissions
        let perms = std::fs::Permissions::from_mode(0o600);
        tokio::fs::set_permissions(&path, perms)
            .await
            .context("Failed to set credentials file permissions")?;

        Ok(())
    }

    /// Get the API key for a specific provider
    pub fn get_api_key(
        creds: &Credentials,
        provider: &super::types::ProviderName,
    ) -> Option<String> {
        match provider {
            super::types::ProviderName::Nebius => creds.nebius_api_key.clone(),
            super::types::ProviderName::Anthropic => creds.anthropic_api_key.clone(),
            super::types::ProviderName::Fireworks => creds.fireworks_api_key.clone(),
            super::types::ProviderName::GithubCopilot => creds.copilot_token.clone(),
            super::types::ProviderName::OpenaiCodex => creds.codex_access_token.clone(),
            super::types::ProviderName::Zai => creds.zai_api_key.clone(),
            super::types::ProviderName::OllamaCloud => creds.ollama_cloud_api_key.clone(),
            super::types::ProviderName::OpenaiCompatible => creds.openai_compatible_api_key.clone(),
            super::types::ProviderName::LukanCloud => creds.lukan_cloud_api_key.clone(),
            super::types::ProviderName::Gemini => creds.gemini_api_key.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::{Credentials, ProviderName};
    use super::*;

    #[test]
    fn test_get_api_key_returns_correct_key_per_provider() {
        let creds = Credentials {
            nebius_api_key: Some("neb-key".into()),
            anthropic_api_key: Some("ant-key".into()),
            fireworks_api_key: Some("fw-key".into()),
            copilot_token: Some("cp-tok".into()),
            codex_access_token: Some("cx-tok".into()),
            zai_api_key: Some("zai-key".into()),
            ollama_cloud_api_key: Some("oll-key".into()),
            openai_compatible_api_key: Some("oai-key".into()),
            lukan_cloud_api_key: Some("lk-key".into()),
            gemini_api_key: Some("gem-key".into()),
            ..Default::default()
        };

        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::Nebius),
            Some("neb-key".into())
        );
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::Anthropic),
            Some("ant-key".into())
        );
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::Fireworks),
            Some("fw-key".into())
        );
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::GithubCopilot),
            Some("cp-tok".into())
        );
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::OpenaiCodex),
            Some("cx-tok".into())
        );
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::Zai),
            Some("zai-key".into())
        );
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::OllamaCloud),
            Some("oll-key".into())
        );
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::OpenaiCompatible),
            Some("oai-key".into())
        );
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::LukanCloud),
            Some("lk-key".into())
        );
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::Gemini),
            Some("gem-key".into())
        );
    }

    #[test]
    fn test_get_api_key_returns_none_when_unset() {
        let creds = Credentials::default();
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::Anthropic),
            None
        );
        assert_eq!(
            CredentialsManager::get_api_key(&creds, &ProviderName::Nebius),
            None
        );
    }
}
