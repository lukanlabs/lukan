use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use lukan_browser::{BrowserConfig, BrowserManager, ProfileMode};
use serde::Serialize;

use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserStatusResponse {
    pub running: bool,
    pub cdp_url: Option<String>,
    pub current_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTabInfo {
    pub id: String,
    pub title: String,
    pub url: String,
    pub ws_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_browser_status_response_serialization() {
        let resp = BrowserStatusResponse {
            running: true,
            cdp_url: Some("ws://localhost:9222".into()),
            current_url: Some("https://example.com".into()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""cdpUrl""#), "cdpUrl camelCase: {json}");
        assert!(
            json.contains(r#""currentUrl""#),
            "currentUrl camelCase: {json}"
        );
        assert!(!json.contains("cdp_url"), "no snake_case: {json}");
        assert!(!json.contains("current_url"), "no snake_case: {json}");
    }

    #[test]
    fn test_browser_status_response_not_running() {
        let resp = BrowserStatusResponse {
            running: false,
            cdp_url: None,
            current_url: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains(r#""running":false"#), "running: {json}");
    }

    #[test]
    fn test_browser_tab_info_serialization() {
        let tab = BrowserTabInfo {
            id: "tab-123".into(),
            title: "Example".into(),
            url: "https://example.com".into(),
            ws_url: "ws://localhost:9222/devtools/page/tab-123".into(),
        };
        let json = serde_json::to_string(&tab).unwrap();
        assert!(json.contains(r#""wsUrl""#), "wsUrl camelCase: {json}");
        assert!(!json.contains("ws_url"), "no snake_case: {json}");
        assert!(json.contains(r#""id":"tab-123""#), "id: {json}");
        assert!(json.contains(r#""title":"Example""#), "title: {json}");
    }
}

/// POST /api/browser/launch
pub async fn browser_launch(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let visible = body["visible"].as_bool().unwrap_or(true);
    let profile = body["profile"].as_str().map(String::from);
    let _port = body["port"].as_u64().map(|p| p as u16);

    if let Some(manager) = BrowserManager::get() {
        if manager.is_active().await {
            return get_browser_status_response().await.into_response();
        }
        manager.reactivate().await;
    }

    let profile_mode = match profile.as_deref() {
        Some("temp") => ProfileMode::Temp,
        Some(path) => ProfileMode::Custom(path.into()),
        None => ProfileMode::Persistent,
    };

    let config = BrowserConfig {
        visible,
        profile: profile_mode,
        browser_name: "auto".to_string(),
        ..Default::default()
    };

    BrowserManager::init(config);

    let manager = match BrowserManager::get() {
        Some(m) => m,
        None => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to init browser").into_response();
        }
    };

    if let Err(e) = manager
        .send_cdp("Runtime.evaluate", serde_json::json!({"expression": "1"}))
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to connect to browser: {e}"),
        )
            .into_response();
    }

    // Hot-reload agent tools with browser tools
    {
        let mut agent_lock = state.agent.lock().await;
        if let Some(agent) = agent_lock.as_mut() {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let project_cfg = lukan_core::config::ProjectConfig::load(&cwd)
                .await
                .ok()
                .flatten()
                .map(|(_, cfg)| cfg);
            let permissions = project_cfg
                .as_ref()
                .map(|c| c.permissions.clone())
                .unwrap_or_default();
            let allowed = project_cfg
                .as_ref()
                .map(|c| c.resolve_allowed_paths(&cwd))
                .unwrap_or_else(|| vec![cwd.clone()]);

            let browser_registry =
                lukan_tools::create_configured_browser_registry(&permissions, &allowed);
            agent.reload_tools(browser_registry);
            agent.enable_browser_tools();

            let prompt = crate::ws_handler::build_system_prompt(true).await;
            agent.reload_system_prompt(prompt);
        }
    }

    get_browser_status_response().await.into_response()
}

/// GET /api/browser/status
pub async fn browser_status() -> impl IntoResponse {
    get_browser_status_response().await.into_response()
}

/// POST /api/browser/navigate
pub async fn browser_navigate(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let url = body["url"].as_str().unwrap_or_default();
    let manager = match BrowserManager::get() {
        Some(m) => m,
        None => return (StatusCode::BAD_REQUEST, "Browser not running").into_response(),
    };

    if let Err(e) = manager
        .send_cdp("Page.navigate", serde_json::json!({"url": url}))
        .await
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Navigation failed: {e}"),
        )
            .into_response();
    }

    let _ = manager
        .wait_for_event("Page.loadEventFired", std::time::Duration::from_secs(10))
        .await;

    match manager.snapshot(true).await {
        Ok(snap) => Json(serde_json::json!(snap)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Snapshot failed: {e}"),
        )
            .into_response(),
    }
}

/// GET /api/browser/screenshot
pub async fn browser_screenshot() -> impl IntoResponse {
    let manager = match BrowserManager::get() {
        Some(m) => m,
        None => return (StatusCode::BAD_REQUEST, "Browser not running").into_response(),
    };
    match manager.quick_screenshot().await {
        Ok(data) => Json(serde_json::json!(data)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Screenshot failed: {e}"),
        )
            .into_response(),
    }
}

/// GET /api/browser/tabs
pub async fn browser_tabs() -> impl IntoResponse {
    let manager = match BrowserManager::get() {
        Some(m) => m,
        None => return (StatusCode::BAD_REQUEST, "Browser not running").into_response(),
    };

    let http_base = match manager.http_base().await {
        Ok(url) => url,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to get HTTP base: {e}"),
            )
                .into_response();
        }
    };

    let url = format!("{http_base}/json/list");
    let resp = match reqwest::get(&url).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to list tabs: {e}"),
            )
                .into_response();
        }
    };

    let targets: Vec<serde_json::Value> = match resp.json().await {
        Ok(t) => t,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to parse tabs: {e}"),
            )
                .into_response();
        }
    };

    let tabs: Vec<BrowserTabInfo> = targets
        .into_iter()
        .filter(|t| t.get("type").and_then(|v| v.as_str()) == Some("page"))
        .map(|t| BrowserTabInfo {
            id: t["id"].as_str().unwrap_or("").to_string(),
            title: t["title"].as_str().unwrap_or("").to_string(),
            url: t["url"].as_str().unwrap_or("").to_string(),
            ws_url: t["webSocketDebuggerUrl"].as_str().unwrap_or("").to_string(),
        })
        .collect();

    Json(tabs).into_response()
}

/// POST /api/browser/close
pub async fn browser_close() -> impl IntoResponse {
    let manager = match BrowserManager::get() {
        Some(m) => m,
        None => return (StatusCode::BAD_REQUEST, "Browser not running").into_response(),
    };
    manager.disconnect().await;
    StatusCode::OK.into_response()
}

async fn get_browser_status_response() -> Json<BrowserStatusResponse> {
    let Some(manager) = BrowserManager::get() else {
        return Json(BrowserStatusResponse {
            running: false,
            cdp_url: None,
            current_url: None,
        });
    };

    if !manager.is_active().await {
        return Json(BrowserStatusResponse {
            running: false,
            cdp_url: None,
            current_url: None,
        });
    }

    let current_url = match manager
        .send_cdp(
            "Runtime.evaluate",
            serde_json::json!({"expression": "window.location.href"}),
        )
        .await
    {
        Ok(result) => result
            .get("result")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.as_str())
            .map(String::from),
        Err(_) => None,
    };

    let cdp_url = manager.http_base().await.ok();

    Json(BrowserStatusResponse {
        running: true,
        cdp_url,
        current_url,
    })
}
