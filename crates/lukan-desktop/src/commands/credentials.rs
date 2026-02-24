use lukan_core::config::{ConfigManager, Credentials, CredentialsManager, ProviderName};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub name: String,
    pub configured: bool,
    pub default_model: String,
}

#[tauri::command]
pub async fn get_credentials() -> Result<Credentials, String> {
    CredentialsManager::load().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_credentials(credentials: Credentials) -> Result<(), String> {
    CredentialsManager::save(&credentials)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_provider_status() -> Result<Vec<ProviderStatus>, String> {
    let creds = CredentialsManager::load()
        .await
        .map_err(|e| e.to_string())?;

    let providers = [
        ProviderName::Anthropic,
        ProviderName::Nebius,
        ProviderName::Fireworks,
        ProviderName::GithubCopilot,
        ProviderName::OpenaiCodex,
        ProviderName::Zai,
        ProviderName::OpenaiCompatible,
    ];

    let statuses = providers
        .iter()
        .map(|p| ProviderStatus {
            name: p.to_string(),
            configured: CredentialsManager::get_api_key(&creds, p).is_some(),
            default_model: p.default_model().to_string(),
        })
        .collect();

    Ok(statuses)
}

#[tauri::command]
pub async fn test_provider(provider: String) -> Result<String, String> {
    let provider_name: ProviderName =
        serde_json::from_value(serde_json::Value::String(provider.clone()))
            .map_err(|_| format!("Invalid provider: {provider}"))?;

    let creds = CredentialsManager::load()
        .await
        .map_err(|e| e.to_string())?;
    let api_key = CredentialsManager::get_api_key(&creds, &provider_name)
        .ok_or_else(|| format!("No API key configured for {provider}"))?;

    let config = ConfigManager::load().await.map_err(|e| e.to_string())?;

    match provider_name {
        ProviderName::Anthropic => {
            let models = lukan_providers::anthropic::fetch_anthropic_models(&api_key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("Connected. {} models available.", models.len()))
        }
        ProviderName::Nebius => {
            let models = lukan_providers::nebius::fetch_nebius_models(&api_key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("Connected. {} models available.", models.len()))
        }
        ProviderName::Fireworks => {
            let models = lukan_providers::fireworks::fetch_fireworks_models(&api_key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("Connected. {} models available.", models.len()))
        }
        ProviderName::GithubCopilot => {
            let models = lukan_providers::github_copilot::fetch_github_copilot_models(&api_key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(format!("Connected. {} models available.", models.len()))
        }
        ProviderName::OpenaiCompatible => {
            let base_url = config
                .openai_compatible_base_url
                .ok_or("No base URL configured for openai-compatible")?;
            Ok(format!("Configured with base URL: {base_url}"))
        }
        _ => Ok(format!("Provider {provider} configured.")),
    }
}
