use std::collections::HashSet;

use lukan_core::config::{AppConfig, ConfigManager};
use tauri::State;

use crate::state::ChatState;

#[tauri::command]
pub async fn get_config() -> Result<AppConfig, String> {
    ConfigManager::load().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_config(state: State<'_, ChatState>, config: AppConfig) -> Result<(), String> {
    ConfigManager::save(&config)
        .await
        .map_err(|e| e.to_string())?;

    // Update cached config so new agents pick up the changes
    if let Some(ref mut resolved) = *state.config.lock().await {
        resolved.config = config.clone();
    }

    // Apply disabled tools to the live agent immediately
    if let Some(ref mut agent) = *state.agent.lock().await {
        let disabled: HashSet<String> = config
            .disabled_tools
            .unwrap_or_default()
            .into_iter()
            .collect();
        agent.set_disabled_tools(disabled);
    }

    Ok(())
}

#[tauri::command]
pub async fn list_tools() -> Result<Vec<lukan_tools::ToolInfo>, String> {
    if lukan_browser::BrowserManager::get().is_some() {
        Ok(lukan_tools::all_tool_info_with_browser())
    } else {
        Ok(lukan_tools::all_tool_info())
    }
}

#[tauri::command]
pub async fn get_config_value(key: String) -> Result<Option<serde_json::Value>, String> {
    ConfigManager::get_value(&key)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_config_value(key: String, value: serde_json::Value) -> Result<(), String> {
    ConfigManager::set_value(&key, value)
        .await
        .map_err(|e| e.to_string())
}
