use std::sync::Arc;

use axum::{Json, extract::Path, extract::State, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use tracing::error;

use lukan_core::config::{ConfigManager, CredentialsManager, ProviderName};
use lukan_providers::create_provider;

use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfoDto {
    pub name: String,
    pub default_model: String,
    pub active: bool,
    pub current_model: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchedModelDto {
    pub id: String,
    pub name: String,
}

/// GET /api/providers
pub async fn list_providers() -> impl IntoResponse {
    let config = match ConfigManager::load().await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

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
        ProviderName::Gemini,
    ];

    let current_model = config.model.clone();
    let list: Vec<ProviderInfoDto> = providers
        .iter()
        .map(|p| ProviderInfoDto {
            name: p.to_string(),
            default_model: String::new(),
            active: config.provider == *p,
            current_model: if config.provider == *p {
                current_model.clone()
            } else {
                None
            },
        })
        .collect();

    Json(list).into_response()
}

/// GET /api/models
pub async fn get_models() -> impl IntoResponse {
    match ConfigManager::get_models().await {
        Ok(models) => Json(models).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/providers/:name/models
pub async fn fetch_provider_models(Path(provider): Path<String>) -> impl IntoResponse {
    let provider_name: ProviderName =
        match serde_json::from_value(serde_json::Value::String(provider.clone())) {
            Ok(p) => p,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid provider: {provider}"),
                )
                    .into_response();
            }
        };

    let creds = match CredentialsManager::load().await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let api_key = CredentialsManager::get_api_key(&creds, &provider_name);
    if api_key.is_none()
        && provider_name != ProviderName::OpenaiCompatible
        && provider_name != ProviderName::OpenaiCodex
        && provider_name != ProviderName::Zai
    {
        return (
            StatusCode::BAD_REQUEST,
            format!("No API key configured for {provider}"),
        )
            .into_response();
    }
    let api_key = api_key.unwrap_or_default();

    let result = match provider_name {
        ProviderName::Anthropic => lukan_providers::anthropic::fetch_anthropic_models(&api_key)
            .await
            .map(|m| {
                m.into_iter()
                    .map(|m| FetchedModelDto {
                        name: m.display_name,
                        id: m.id,
                    })
                    .collect::<Vec<_>>()
            }),
        ProviderName::Nebius => lukan_providers::nebius::fetch_nebius_models(&api_key)
            .await
            .map(|m| {
                m.into_iter()
                    .map(|m| FetchedModelDto {
                        name: m.id.clone(),
                        id: m.id,
                    })
                    .collect()
            }),
        ProviderName::Fireworks => lukan_providers::fireworks::fetch_fireworks_models(&api_key)
            .await
            .map(|m| {
                m.into_iter()
                    .map(|m| FetchedModelDto {
                        name: m.display_name,
                        id: m.id,
                    })
                    .collect()
            }),
        ProviderName::GithubCopilot => {
            let mgr = lukan_providers::copilot_token::CopilotTokenManager::new(api_key);
            lukan_providers::github_copilot::fetch_github_copilot_models(&mgr)
                .await
                .map(|m| {
                    m.into_iter()
                        .map(|id| FetchedModelDto {
                            name: id.clone(),
                            id,
                        })
                        .collect()
                })
        }
        ProviderName::OpenaiCompatible => {
            let config = match ConfigManager::load().await {
                Ok(c) => c,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
                }
            };
            match config
                .openai_compatible_base_url
                .as_ref()
                .filter(|s| !s.trim().is_empty())
            {
                Some(base_url) => lukan_providers::openai_compat::fetch_openai_compatible_models(
                    base_url, &api_key,
                )
                .await
                .map(|m| {
                    m.into_iter()
                        .map(|id| FetchedModelDto {
                            name: id.clone(),
                            id,
                        })
                        .collect()
                }),
                None => Err(anyhow::anyhow!(
                    "No base URL configured for openai-compatible"
                )),
            }
        }
        ProviderName::OpenaiCodex => Ok(lukan_providers::openai_codex::codex_models()
            .into_iter()
            .map(|id| FetchedModelDto {
                name: id.clone(),
                id,
            })
            .collect()),
        ProviderName::Zai => Ok(vec![
            FetchedModelDto {
                id: "glm-5".into(),
                name: "GLM-5".into(),
            },
            FetchedModelDto {
                id: "glm-4.7".into(),
                name: "GLM-4.7".into(),
            },
            FetchedModelDto {
                id: "glm-4.6".into(),
                name: "GLM-4.6".into(),
            },
            FetchedModelDto {
                id: "glm-4.5".into(),
                name: "GLM-4.5".into(),
            },
            FetchedModelDto {
                id: "glm-4.5v".into(),
                name: "GLM-4.5V (vision)".into(),
            },
            FetchedModelDto {
                id: "glm-4.1v".into(),
                name: "GLM-4.1V (vision)".into(),
            },
            FetchedModelDto {
                id: "glm-4".into(),
                name: "GLM-4".into(),
            },
        ]),
        ProviderName::LukanCloud => {
            lukan_providers::lukan_cloud::fetch_lukan_cloud_models(&api_key)
                .await
                .map(|m| {
                    m.into_iter()
                        .map(|m| FetchedModelDto {
                            name: format!("{} ({})", m.name, m.tier),
                            id: m.id,
                        })
                        .collect()
                })
        }
        ProviderName::Gemini => lukan_providers::gemini::fetch_gemini_models(&api_key)
            .await
            .map(|m| {
                m.into_iter()
                    .map(|m| FetchedModelDto {
                        name: m.display_name,
                        id: m.id,
                    })
                    .collect()
            }),
        _ => Ok(vec![]),
    };

    match result {
        Ok(models) => Json(models).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/providers/active
pub async fn set_active_provider(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let provider_str = body["provider"].as_str().unwrap_or_default().to_string();
    let model_raw = body["model"].as_str().map(|s| s.to_string());

    let provider_name: ProviderName =
        match serde_json::from_value(serde_json::Value::String(provider_str.clone())) {
            Ok(p) => p,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("Invalid provider: {provider_str}"),
                )
                    .into_response();
            }
        };

    let model_str = model_raw.map(|m| {
        if let Some((_prefix, raw)) = m.split_once(':') {
            raw.to_string()
        } else {
            m
        }
    });

    // 1. Save to disk config
    let mut disk_config = match ConfigManager::load().await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    disk_config.provider = provider_name.clone();
    disk_config.model = model_str.clone();
    if let Err(e) = ConfigManager::save(&disk_config).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    // 2. Hot-swap provider on running agents (same as WS handle_set_model)
    {
        let mut config = state.config.lock().await;
        config.config.provider = provider_name;
        config.config.model = model_str.clone();

        match create_provider(&config) {
            Ok(new_provider) => {
                let new_provider: Arc<dyn lukan_providers::Provider> = Arc::from(new_provider);

                // Swap on legacy agent
                {
                    let mut agent_lock = state.agent.lock().await;
                    if let Some(ref mut agent) = *agent_lock {
                        agent.swap_provider(Arc::clone(&new_provider));
                    }
                }

                // Swap on all session agents
                {
                    let mut sessions = state.sessions.lock().await;
                    for session in sessions.values_mut() {
                        if let Some(ref mut agent) = session.agent
                            && let Ok(p) = create_provider(&config)
                        {
                            agent.swap_provider(Arc::from(p));
                        }
                    }
                }
            }
            Err(e) => {
                error!("Failed to create provider for hot-swap: {e}");
                // Config was saved, but agent won't use new model until restart
            }
        }
    }

    // 3. Update state names
    *state.provider_name.lock().await = provider_str;
    *state.model_name.lock().await = model_str.unwrap_or_default();

    StatusCode::OK.into_response()
}

/// POST /api/models
pub async fn add_model(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let entry = body["entry"].as_str().unwrap_or_default();
    match ConfigManager::add_model(entry).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/providers/:name/models
pub async fn set_provider_models(
    Path(provider): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let entries: Vec<String> = body["entries"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let vision_ids: Vec<String> = body["visionIds"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    match ConfigManager::set_provider_models(&provider, &entries, &vision_ids).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
