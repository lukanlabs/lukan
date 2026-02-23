//! Transitional module for WhatsApp-specific logic.
//!
//! Contains the system prompt builder and config loader used by `plugin.rs`
//! when starting the "whatsapp" plugin. Will be generalized in the future.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use lukan_core::config::{LukanPaths, PluginOverrides, WA_DEFAULT_TOOLS, WhatsAppConfig};
use lukan_providers::SystemPrompt;

const BASE_PROMPT: &str = include_str!("../prompts/base.txt");

const WA_PLUGIN_NAME: &str = "whatsapp";

/// Plugin-specific configuration for WhatsApp.
/// Lives in ~/.config/lukan/plugins/whatsapp/config.json.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WhatsAppPluginConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bridge_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whitelist: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_groups: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_dirs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_dir_restrictions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder_advance: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder_chat: Option<String>,
}

/// Load the WhatsApp plugin's config.json
async fn load_wa_plugin_config() -> Result<WhatsAppPluginConfig> {
    let config_path = LukanPaths::plugin_config(WA_PLUGIN_NAME);
    if !config_path.exists() {
        return Ok(WhatsAppPluginConfig::default());
    }
    let content = tokio::fs::read_to_string(&config_path).await?;
    let config: WhatsAppPluginConfig = serde_json::from_str(&content)?;
    Ok(config)
}

/// Convert WhatsAppPluginConfig to the legacy WhatsAppConfig (for system prompt builder etc.)
fn plugin_config_to_wa_config(
    pc: &WhatsAppPluginConfig,
    overrides: Option<&PluginOverrides>,
) -> WhatsAppConfig {
    WhatsAppConfig {
        enabled: pc.enabled,
        bridge_url: pc.bridge_url.clone(),
        whitelist: pc.whitelist.clone(),
        allowed_groups: pc.allowed_groups.clone(),
        prefix: pc.prefix.clone(),
        tools: pc.tools.clone(),
        allowed_dirs: pc.allowed_dirs.clone(),
        skip_dir_restrictions: pc.skip_dir_restrictions,
        provider: overrides.and_then(|o| o.provider.clone()),
        model: overrides.and_then(|o| o.model.clone()),
        reminder_advance: pc.reminder_advance,
        reminder_chat: pc.reminder_chat.clone(),
    }
}

/// Load WhatsApp plugin config and build a WhatsAppConfig suitable for system prompt building.
/// Called by plugin.rs when starting the "whatsapp" plugin.
pub async fn load_wa_config_for_plugin(
    overrides: Option<&PluginOverrides>,
) -> Result<WhatsAppConfig> {
    let pc = load_wa_plugin_config().await?;
    Ok(plugin_config_to_wa_config(&pc, overrides))
}

/// Build the WhatsApp system prompt — structured like the CLI prompt
/// (base + memory + plugin prompts + WA-specific sections + dynamic date).
pub async fn build_whatsapp_system_prompt(
    wa_config: &WhatsAppConfig,
    timezone: Option<&str>,
) -> SystemPrompt {
    let mut cached = vec![BASE_PROMPT.to_string()];

    // Global memory
    let global_path = LukanPaths::global_memory_file();
    if let Ok(memory) = tokio::fs::read_to_string(&global_path).await {
        let trimmed = memory.trim();
        if !trimmed.is_empty() {
            cached.push(format!("## Global Memory\n\n{trimmed}"));
        }
    }

    // Project memory (if active)
    let active_path = LukanPaths::project_memory_active_file();
    if tokio::fs::metadata(&active_path).await.is_ok() {
        let project_path = LukanPaths::project_memory_file();
        if let Ok(memory) = tokio::fs::read_to_string(&project_path).await {
            let trimmed = memory.trim();
            if !trimmed.is_empty() {
                cached.push(format!("## Project Memory\n\n{trimmed}"));
            }
        }
    }

    // Plugin prompts (prompt.txt from all installed plugins)
    let plugins_dir = LukanPaths::plugins_dir();
    if let Ok(mut entries) = tokio::fs::read_dir(&plugins_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let prompt_path = entry.path().join("prompt.txt");
            if let Ok(plugin_prompt) = tokio::fs::read_to_string(&prompt_path).await {
                let trimmed = plugin_prompt.trim();
                if !trimmed.is_empty() {
                    cached.push(trimmed.to_string());
                }
            }
        }
    }

    // WhatsApp-specific: directory restrictions (read templates from plugin dir)
    let tools: Vec<String> = wa_config
        .tools
        .clone()
        .unwrap_or_else(|| WA_DEFAULT_TOOLS.iter().map(|s| s.to_string()).collect());

    let has_dangerous = tools
        .iter()
        .any(|t| t == "Bash" || t == "WriteFile" || t == "EditFile");

    if has_dangerous && !wa_config.skip_dir_restrictions.unwrap_or(false) {
        let dirs = wa_config.allowed_dirs.clone().unwrap_or_default();
        let wa_plugin_dir = LukanPaths::plugins_dir().join("whatsapp");
        if dirs.is_empty() {
            let path = wa_plugin_dir.join("prompt-dir-none.txt");
            if let Ok(text) = tokio::fs::read_to_string(&path).await {
                cached.push(text);
            }
        } else {
            let dir_list = dirs
                .iter()
                .map(|d| format!("- `{d}`"))
                .collect::<Vec<_>>()
                .join("\n");
            let path = wa_plugin_dir.join("prompt-dir-allowed.txt");
            if let Ok(text) = tokio::fs::read_to_string(&path).await {
                cached.push(text.replace("{{ALLOWED_DIRS}}", &dir_list));
            }
        }
    }

    // Dynamic: current date/time
    let now = chrono::Utc::now();
    let tz_name = timezone.unwrap_or("UTC");
    let dynamic = format!(
        "Current date: {} ({}). Use this for any time-relative operations.",
        now.format("%Y-%m-%d %H:%M UTC"),
        tz_name
    );

    SystemPrompt::Structured { cached, dynamic }
}
