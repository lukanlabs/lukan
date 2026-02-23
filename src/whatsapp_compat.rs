//! Transitional module for WhatsApp-specific logic.
//!
//! Contains the system prompt builder and config loader used by `plugin.rs`
//! when starting the "whatsapp" plugin. Will be generalized in the future.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use lukan_core::config::{LukanPaths, PluginOverrides, WhatsAppConfig, WA_DEFAULT_TOOLS};
use lukan_providers::SystemPrompt;

const WA_FORMAT_PROMPT: &str = include_str!("../prompts/whatsapp-format.txt");
const WA_DIR_NONE_PROMPT: &str = include_str!("../prompts/whatsapp-dir-none.txt");
const WA_DIR_ALLOWED_PROMPT: &str = include_str!("../prompts/whatsapp-dir-allowed.txt");
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

/// Build the WhatsApp-specific system prompt with directory restrictions.
pub fn build_whatsapp_system_prompt(wa_config: &WhatsAppConfig) -> SystemPrompt {
    let mut prompt = String::new();
    prompt.push_str(BASE_PROMPT);
    prompt.push_str("\n\n");
    prompt.push_str(WA_FORMAT_PROMPT);

    let tools: Vec<String> = wa_config
        .tools
        .clone()
        .unwrap_or_else(|| WA_DEFAULT_TOOLS.iter().map(|s| s.to_string()).collect());

    let has_dangerous = tools
        .iter()
        .any(|t| t == "Bash" || t == "WriteFile" || t == "EditFile");

    if has_dangerous && !wa_config.skip_dir_restrictions.unwrap_or(false) {
        let dirs = wa_config.allowed_dirs.clone().unwrap_or_default();
        if dirs.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(WA_DIR_NONE_PROMPT);
        } else {
            prompt.push_str("\n\n");
            let dir_list = dirs
                .iter()
                .map(|d| format!("- `{d}`"))
                .collect::<Vec<_>>()
                .join("\n");
            prompt.push_str(&WA_DIR_ALLOWED_PROMPT.replace("{{ALLOWED_DIRS}}", &dir_list));
        }
    }

    SystemPrompt::Text(prompt)
}
