use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

use lukan_core::config::{AppConfig, ConfigManager};

use crate::state::AppState;

/// GET /api/config
pub async fn get_config() -> impl IntoResponse {
    match ConfigManager::load().await {
        Ok(config) => Json(serde_json::to_value(config).unwrap()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/config
pub async fn save_config(
    State(state): State<Arc<AppState>>,
    Json(config): Json<AppConfig>,
) -> impl IntoResponse {
    if let Err(e) = ConfigManager::save(&config).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    // Update cached config
    {
        let mut resolved = state.config.lock().await;
        resolved.config = config.clone();
    }

    // Apply disabled tools to live agent
    {
        let mut agent_lock = state.agent.lock().await;
        if let Some(agent) = agent_lock.as_mut() {
            let disabled: HashSet<String> = config
                .disabled_tools
                .unwrap_or_default()
                .into_iter()
                .collect();
            agent.set_disabled_tools(disabled);
        }
    }

    StatusCode::OK.into_response()
}

/// GET /api/config/:key
pub async fn get_config_value(Path(key): Path<String>) -> impl IntoResponse {
    match ConfigManager::get_value(&key).await {
        Ok(val) => Json(serde_json::json!({ "value": val })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/config/:key
pub async fn set_config_value(
    Path(key): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let value = body
        .get("value")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    match ConfigManager::set_value(&key, value).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/tools
pub async fn list_tools() -> Json<Vec<lukan_tools::ToolInfo>> {
    if lukan_browser::BrowserManager::get().is_some() {
        Json(lukan_tools::all_tool_info_with_browser())
    } else {
        Json(lukan_tools::all_tool_info())
    }
}
