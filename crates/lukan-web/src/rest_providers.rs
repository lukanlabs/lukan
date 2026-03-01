use axum::{Json, extract::Path, http::StatusCode, response::IntoResponse};
use serde::Serialize;

use lukan_core::config::{ConfigManager, CredentialsManager, ProviderName};

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
        ProviderName::OpenaiCompatible,
    ];

    let current_model = config.model.clone();
    let list: Vec<ProviderInfoDto> = providers
        .iter()
        .map(|p| ProviderInfoDto {
            name: p.to_string(),
            default_model: p.default_model().to_string(),
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
    if api_key.is_none() && provider_name != ProviderName::OpenaiCompatible {
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
            lukan_providers::github_copilot::fetch_github_copilot_models(&api_key)
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
        _ => Ok(vec![FetchedModelDto {
            id: provider_name.default_model().to_string(),
            name: provider_name.default_model().to_string(),
        }]),
    };

    match result {
        Ok(models) => Json(models).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/providers/active
pub async fn set_active_provider(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let provider = body["provider"].as_str().unwrap_or_default().to_string();
    let model = body["model"].as_str().map(|s| s.to_string());

    let mut config = match ConfigManager::load().await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    config.provider = match serde_json::from_value(serde_json::Value::String(provider.clone())) {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Invalid provider: {provider}"),
            )
                .into_response();
        }
    };

    config.model = model.map(|m| {
        if let Some((_prefix, raw)) = m.split_once(':') {
            raw.to_string()
        } else {
            m
        }
    });

    match ConfigManager::save(&config).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
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
