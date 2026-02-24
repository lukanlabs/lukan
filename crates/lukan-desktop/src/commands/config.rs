use lukan_core::config::{AppConfig, ConfigManager};

#[tauri::command]
pub async fn get_config() -> Result<AppConfig, String> {
    ConfigManager::load().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_config(config: AppConfig) -> Result<(), String> {
    ConfigManager::save(&config)
        .await
        .map_err(|e| e.to_string())
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
