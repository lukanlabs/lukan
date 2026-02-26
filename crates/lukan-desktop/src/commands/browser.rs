use std::path::PathBuf;

use serde::Serialize;
use tauri::State;

use lukan_browser::{BrowserConfig, BrowserManager, ProfileMode};
use lukan_tools::create_configured_browser_registry;

use crate::state::{ChatState, build_system_prompt};

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

/// Launch Chrome with visible mode and persistent profile.
/// Also hot-reloads the agent's tool registry + permissions so browser tools
/// become available in the current chat session.
#[tauri::command]
pub async fn browser_launch(
    state: State<'_, ChatState>,
    visible: Option<bool>,
    profile: Option<String>,
    port: Option<u16>,
) -> Result<BrowserStatusResponse, String> {
    // If already initialized and still active, just return status
    if let Some(manager) = BrowserManager::get() {
        if manager.is_active().await {
            return browser_status().await;
        }
        // Was disconnected — reactivate so ensure_connected() can auto-launch
        manager.reactivate().await;
    }

    let profile_mode = match profile.as_deref() {
        Some("temp") => ProfileMode::Temp,
        Some(path) => ProfileMode::Custom(path.into()),
        None => ProfileMode::Persistent,
    };

    let config = BrowserConfig {
        visible: visible.unwrap_or(true),
        profile: profile_mode,
        browser_name: "auto".to_string(),
        ..Default::default()
    };

    // Set custom port if provided
    if let Some(_port) = port {
        // Port is handled by ChromeOptions, not BrowserConfig directly.
        // BrowserManager auto-launches with default port.
    }

    BrowserManager::init(config);

    // Force connection to actually launch Chrome
    let manager = BrowserManager::get().ok_or("Failed to get BrowserManager after init")?;
    manager
        .send_cdp("Runtime.evaluate", serde_json::json!({"expression": "1"}))
        .await
        .map_err(|e| format!("Failed to connect to browser: {e}"))?;

    // Hot-reload the live agent with browser tools
    {
        let mut agent_lock = state.agent.lock().await;
        if let Some(agent) = agent_lock.as_mut() {
            // 1. Swap tool registry to include browser tools
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
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

            let browser_registry = create_configured_browser_registry(&permissions, &allowed);
            agent.reload_tools(browser_registry);

            // 2. Enable browser tools in the permission matcher (auto-allow)
            agent.enable_browser_tools();

            // 3. Reload system prompt with browser tool instructions
            let prompt = build_system_prompt(true).await;
            agent.reload_system_prompt(prompt);
        }
    }

    browser_status().await
}

/// Get browser status without triggering auto-connect/auto-launch.
#[tauri::command]
pub async fn browser_status() -> Result<BrowserStatusResponse, String> {
    let Some(manager) = BrowserManager::get() else {
        return Ok(BrowserStatusResponse {
            running: false,
            cdp_url: None,
            current_url: None,
        });
    };

    // Check if browser is active without auto-connecting
    if !manager.is_active().await {
        return Ok(BrowserStatusResponse {
            running: false,
            cdp_url: None,
            current_url: None,
        });
    }

    // Browser is active — safe to query via CDP
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

    Ok(BrowserStatusResponse {
        running: true,
        cdp_url,
        current_url,
    })
}

/// Navigate to a URL and return accessibility snapshot.
#[tauri::command]
pub async fn browser_navigate(url: String) -> Result<String, String> {
    let manager = BrowserManager::get().ok_or("Browser not running. Launch it first.")?;

    manager
        .send_cdp("Page.navigate", serde_json::json!({"url": url}))
        .await
        .map_err(|e| format!("Navigation failed: {e}"))?;

    // Wait for load
    let _ = manager
        .wait_for_event("Page.loadEventFired", std::time::Duration::from_secs(10))
        .await;

    // Return accessibility snapshot
    manager
        .snapshot(true)
        .await
        .map_err(|e| format!("Snapshot failed: {e}"))
}

/// Take a screenshot and return as base64 data URL.
#[tauri::command]
pub async fn browser_screenshot() -> Result<String, String> {
    let manager = BrowserManager::get().ok_or("Browser not running. Launch it first.")?;
    manager
        .quick_screenshot()
        .await
        .map_err(|e| format!("Screenshot failed: {e}"))
}

/// List open browser tabs.
#[tauri::command]
pub async fn browser_tabs() -> Result<Vec<BrowserTabInfo>, String> {
    let manager = BrowserManager::get().ok_or("Browser not running. Launch it first.")?;

    let http_base = manager
        .http_base()
        .await
        .map_err(|e| format!("Failed to get HTTP base: {e}"))?;

    // GET /json/list to enumerate targets
    let url = format!("{http_base}/json/list");
    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("Failed to list tabs: {e}"))?;
    let targets: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse tabs: {e}"))?;

    let tabs = targets
        .into_iter()
        .filter(|t| t.get("type").and_then(|v| v.as_str()) == Some("page"))
        .map(|t| BrowserTabInfo {
            id: t
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            title: t
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            url: t
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            ws_url: t
                .get("webSocketDebuggerUrl")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        })
        .collect();

    Ok(tabs)
}

/// Disconnect and kill Chrome.
#[tauri::command]
pub async fn browser_close() -> Result<(), String> {
    let manager = BrowserManager::get().ok_or("Browser not running.")?;
    manager.disconnect().await;
    Ok(())
}
