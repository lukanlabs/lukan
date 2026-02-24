use lukan_core::config::{ConfigManager, CredentialsManager, ProviderName};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub name: String,
    pub default_model: String,
    pub active: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedModel {
    pub id: String,
    pub name: String,
}

#[tauri::command]
pub async fn list_providers() -> Result<Vec<ProviderInfo>, String> {
    let config = ConfigManager::load().await.map_err(|e| e.to_string())?;

    let providers = [
        ProviderName::Anthropic,
        ProviderName::Nebius,
        ProviderName::Fireworks,
        ProviderName::GithubCopilot,
        ProviderName::OpenaiCodex,
        ProviderName::Zai,
        ProviderName::OpenaiCompatible,
    ];

    Ok(providers
        .iter()
        .map(|p| ProviderInfo {
            name: p.to_string(),
            default_model: p.default_model().to_string(),
            active: config.provider == *p,
        })
        .collect())
}

#[tauri::command]
pub async fn get_models() -> Result<Vec<String>, String> {
    ConfigManager::get_models().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_provider_models(provider: String) -> Result<Vec<FetchedModel>, String> {
    let provider_name: ProviderName =
        serde_json::from_value(serde_json::Value::String(provider.clone()))
            .map_err(|_| format!("Invalid provider: {provider}"))?;

    let creds = CredentialsManager::load()
        .await
        .map_err(|e| e.to_string())?;
    let api_key = CredentialsManager::get_api_key(&creds, &provider_name)
        .ok_or_else(|| format!("No API key configured for {provider}"))?;

    match provider_name {
        ProviderName::Anthropic => {
            let models = lukan_providers::anthropic::fetch_anthropic_models(&api_key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(models
                .into_iter()
                .map(|m| FetchedModel {
                    name: m.display_name,
                    id: m.id,
                })
                .collect())
        }
        ProviderName::Nebius => {
            let models = lukan_providers::nebius::fetch_nebius_models(&api_key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(models
                .into_iter()
                .map(|m| FetchedModel {
                    name: m.id.clone(),
                    id: m.id,
                })
                .collect())
        }
        ProviderName::Fireworks => {
            let models = lukan_providers::fireworks::fetch_fireworks_models(&api_key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(models
                .into_iter()
                .map(|m| FetchedModel {
                    name: m.display_name,
                    id: m.id,
                })
                .collect())
        }
        ProviderName::GithubCopilot => {
            let models = lukan_providers::github_copilot::fetch_github_copilot_models(&api_key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(models
                .into_iter()
                .map(|id| FetchedModel {
                    name: id.clone(),
                    id,
                })
                .collect())
        }
        _ => Ok(vec![FetchedModel {
            id: provider_name.default_model().to_string(),
            name: provider_name.default_model().to_string(),
        }]),
    }
}

#[tauri::command]
pub async fn set_active_provider(provider: String, model: Option<String>) -> Result<(), String> {
    let mut config = ConfigManager::load().await.map_err(|e| e.to_string())?;

    config.provider = serde_json::from_value(serde_json::Value::String(provider.clone()))
        .map_err(|_| format!("Invalid provider: {provider}"))?;
    config.model = model;

    ConfigManager::save(&config)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_model(entry: String) -> Result<(), String> {
    ConfigManager::add_model(&entry)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_provider_models(
    provider: String,
    entries: Vec<String>,
    vision_ids: Vec<String>,
) -> Result<(), String> {
    ConfigManager::set_provider_models(&provider, &entries, &vision_ids)
        .await
        .map_err(|e| e.to_string())
}
