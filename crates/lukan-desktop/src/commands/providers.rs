use lukan_core::config::{ConfigManager, CredentialsManager, ProviderName};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub name: String,
    pub default_model: String,
    pub active: bool,
    /// The currently configured model (if set), stripped of provider prefix.
    pub current_model: Option<String>,
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
        ProviderName::OllamaCloud,
        ProviderName::OpenaiCompatible,
        ProviderName::LukanCloud,
    ];

    let current_model = config.model.clone();
    Ok(providers
        .iter()
        .map(|p| ProviderInfo {
            name: p.to_string(),
            default_model: String::new(),
            active: config.provider == *p,
            current_model: if config.provider == *p {
                current_model.clone()
            } else {
                None
            },
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
    let api_key = CredentialsManager::get_api_key(&creds, &provider_name);

    // Some providers don't need an API key to list models (static lists or local servers)
    if api_key.is_none()
        && provider_name != ProviderName::OpenaiCompatible
        && provider_name != ProviderName::OpenaiCodex
        && provider_name != ProviderName::Zai
    {
        return Err(format!("No API key configured for {provider}"));
    }
    let api_key = api_key.unwrap_or_default();

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
            let mgr = lukan_providers::copilot_token::CopilotTokenManager::new(api_key);
            let models = lukan_providers::github_copilot::fetch_github_copilot_models(&mgr)
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
        ProviderName::OllamaCloud => {
            let models = lukan_providers::ollama_cloud::fetch_ollama_cloud_models(&api_key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(models
                .into_iter()
                .map(|m| FetchedModel {
                    name: m.name,
                    id: m.model,
                })
                .collect())
        }
        ProviderName::OpenaiCompatible => {
            let config = lukan_core::config::ConfigManager::load()
                .await
                .map_err(|e| e.to_string())?;
            let base_url = config
                .openai_compatible_base_url
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| {
                    "No base URL configured for openai-compatible. Set it in Config > OpenAI Compatible.".to_string()
                })?;
            let models =
                lukan_providers::openai_compat::fetch_openai_compatible_models(base_url, &api_key)
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
        ProviderName::OpenaiCodex => Ok(lukan_providers::openai_codex::codex_models()
            .into_iter()
            .map(|id| FetchedModel {
                name: id.clone(),
                id,
            })
            .collect()),
        ProviderName::LukanCloud => {
            let models = lukan_providers::lukan_cloud::fetch_lukan_cloud_models(&api_key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(models
                .into_iter()
                .map(|m| FetchedModel {
                    name: format!("{} ({})", m.name, m.tier),
                    id: m.id,
                })
                .collect())
        }
        ProviderName::Zai => Ok(vec![
            FetchedModel {
                id: "glm-5".into(),
                name: "GLM-5".into(),
            },
            FetchedModel {
                id: "glm-4.7".into(),
                name: "GLM-4.7".into(),
            },
            FetchedModel {
                id: "glm-4.6".into(),
                name: "GLM-4.6".into(),
            },
            FetchedModel {
                id: "glm-4.5".into(),
                name: "GLM-4.5".into(),
            },
            FetchedModel {
                id: "glm-4.5v".into(),
                name: "GLM-4.5V (vision)".into(),
            },
            FetchedModel {
                id: "glm-4.1v".into(),
                name: "GLM-4.1V (vision)".into(),
            },
            FetchedModel {
                id: "glm-4".into(),
                name: "GLM-4".into(),
            },
        ]),
    }
}

#[tauri::command]
pub async fn set_active_provider(provider: String, model: Option<String>) -> Result<(), String> {
    let mut config = ConfigManager::load().await.map_err(|e| e.to_string())?;

    config.provider = serde_json::from_value(serde_json::Value::String(provider.clone()))
        .map_err(|_| format!("Invalid provider: {provider}"))?;

    // Models from getModels() are stored as "provider:model_id" — strip only the known prefix
    config.model = model.map(|m| {
        let prefix = format!("{}:", provider);
        if m.starts_with(&prefix) {
            m[prefix.len()..].to_string()
        } else {
            m
        }
    });

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
