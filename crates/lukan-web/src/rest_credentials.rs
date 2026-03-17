use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Serialize;

use crate::state::AppState;
use lukan_core::config::{ConfigManager, Credentials, CredentialsManager, ProviderName};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatusDto {
    pub name: String,
    pub configured: bool,
    pub default_model: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_status_dto_serialization() {
        let dto = ProviderStatusDto {
            name: "anthropic".into(),
            configured: true,
            default_model: "claude-sonnet-4-20250514".into(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(
            json.contains(r#""defaultModel""#),
            "defaultModel camelCase: {json}"
        );
        assert!(!json.contains("default_model"), "no snake_case: {json}");
        assert!(json.contains(r#""configured":true"#), "configured: {json}");
    }
}

/// GET /api/credentials
pub async fn get_credentials() -> impl IntoResponse {
    match CredentialsManager::load().await {
        Ok(creds) => Json(serde_json::to_value(creds).unwrap()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/credentials
pub async fn save_credentials(
    State(state): State<Arc<AppState>>,
    Json(credentials): Json<Credentials>,
) -> impl IntoResponse {
    match CredentialsManager::save(&credentials).await {
        Ok(()) => {
            // Update in-memory config so create_agent() uses fresh credentials
            state.config.lock().await.credentials = credentials;
            StatusCode::OK.into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/providers/status
pub async fn get_provider_status() -> impl IntoResponse {
    let creds = match CredentialsManager::load().await {
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
        ProviderName::LukanCloud,
        ProviderName::OpenaiCompatible,
        ProviderName::Gemini,
    ];

    let statuses: Vec<ProviderStatusDto> = providers
        .iter()
        .map(|p| ProviderStatusDto {
            name: p.to_string(),
            configured: CredentialsManager::get_api_key(&creds, p).is_some(),
            default_model: String::new(),
        })
        .collect();

    Json(statuses).into_response()
}

/// POST /api/providers/:name/test
pub async fn test_provider(Path(provider): Path<String>) -> impl IntoResponse {
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

    let api_key = match CredentialsManager::get_api_key(&creds, &provider_name) {
        Some(k) => k,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!("No API key configured for {provider}"),
            )
                .into_response();
        }
    };

    let config = match ConfigManager::load().await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let result = match provider_name {
        ProviderName::Anthropic => lukan_providers::anthropic::fetch_anthropic_models(&api_key)
            .await
            .map(|m| format!("Connected. {} models available.", m.len())),
        ProviderName::Nebius => lukan_providers::nebius::fetch_nebius_models(&api_key)
            .await
            .map(|m| format!("Connected. {} models available.", m.len())),
        ProviderName::Fireworks => lukan_providers::fireworks::fetch_fireworks_models(&api_key)
            .await
            .map(|m| format!("Connected. {} models available.", m.len())),
        ProviderName::GithubCopilot => {
            let mgr = lukan_providers::copilot_token::CopilotTokenManager::new(api_key);
            lukan_providers::github_copilot::fetch_github_copilot_models(&mgr)
                .await
                .map(|m| format!("Connected. {} models available.", m.len()))
        }
        ProviderName::OpenaiCompatible => match config.openai_compatible_base_url {
            Some(url) => Ok(format!("Configured with base URL: {url}")),
            None => Err(anyhow::anyhow!(
                "No base URL configured for openai-compatible"
            )),
        },
        ProviderName::Gemini => lukan_providers::gemini::fetch_gemini_models(&api_key)
            .await
            .map(|m| format!("Connected. {} models available.", m.len())),
        _ => Ok(format!("Provider {provider} configured.")),
    };

    match result {
        Ok(msg) => Json(serde_json::json!({ "message": msg })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
