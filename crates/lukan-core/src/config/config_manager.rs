use anyhow::{Context, Result};
use tracing::debug;

use super::paths::LukanPaths;
use super::types::AppConfig;

/// Manages loading and saving the main application config
pub struct ConfigManager;

impl ConfigManager {
    /// Load config from disk, falling back to defaults
    pub async fn load() -> Result<AppConfig> {
        let path = LukanPaths::config_file();

        if !path.exists() {
            debug!("No config file found, using defaults");
            return Ok(AppConfig::default());
        }

        let content = tokio::fs::read_to_string(&path)
            .await
            .context("Failed to read config file")?;

        let config: AppConfig =
            serde_json::from_str(&content).context("Failed to parse config.json")?;

        Ok(config)
    }

    /// Save config to disk (merges with existing)
    pub async fn save(config: &AppConfig) -> Result<()> {
        LukanPaths::ensure_dirs().await?;
        let path = LukanPaths::config_file();
        let content = serde_json::to_string_pretty(config)?;
        tokio::fs::write(&path, content)
            .await
            .context("Failed to write config file")?;
        Ok(())
    }

    /// Get a config value by dot-separated key path
    pub async fn get_value(key: &str) -> Result<Option<serde_json::Value>> {
        let config = Self::load().await?;
        let json = serde_json::to_value(&config)?;
        let value = get_nested_value(&json, key);
        Ok(value)
    }

    /// Set a config value by dot-separated key path
    pub async fn set_value(key: &str, value: serde_json::Value) -> Result<()> {
        let config = Self::load().await?;
        let mut json = serde_json::to_value(&config)?;
        set_nested_value(&mut json, key, value);
        let updated: AppConfig = serde_json::from_value(json)?;
        Self::save(&updated).await
    }
}

/// Get a value from a JSON object by dot-separated path
fn get_nested_value(json: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = json;

    for part in parts {
        match current.get(part) {
            Some(v) => current = v,
            None => return None,
        }
    }

    Some(current.clone())
}

/// Set a value in a JSON object by dot-separated path
fn set_nested_value(json: &mut serde_json::Value, path: &str, value: serde_json::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = json;

    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            if let Some(obj) = current.as_object_mut() {
                obj.insert(part.to_string(), value);
            }
            return;
        }

        if (current.get(part).is_none() || !current[part].is_object())
            && let Some(obj) = current.as_object_mut()
        {
            obj.insert(part.to_string(), serde_json::json!({}));
        }
        current = &mut current[part];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nested_value_get() {
        let json = serde_json::json!({
            "provider": "anthropic",
            "whatsapp": {
                "enabled": true,
                "bridgeUrl": "ws://localhost:3001"
            }
        });

        assert_eq!(
            get_nested_value(&json, "provider"),
            Some(serde_json::json!("anthropic"))
        );
        assert_eq!(
            get_nested_value(&json, "whatsapp.enabled"),
            Some(serde_json::json!(true))
        );
        assert_eq!(get_nested_value(&json, "nonexistent"), None);
    }

    #[test]
    fn test_nested_value_set() {
        let mut json = serde_json::json!({
            "provider": "nebius"
        });

        set_nested_value(&mut json, "provider", serde_json::json!("anthropic"));
        assert_eq!(json["provider"], serde_json::json!("anthropic"));

        set_nested_value(&mut json, "whatsapp.enabled", serde_json::json!(true));
        assert_eq!(json["whatsapp"]["enabled"], serde_json::json!(true));
    }
}
