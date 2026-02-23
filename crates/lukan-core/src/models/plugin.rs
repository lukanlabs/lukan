use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Protocol version for host ↔ plugin communication
pub const PROTOCOL_VERSION: u32 = 1;

// ── Config schema types ──────────────────────────────────────────────

/// Supported configuration field types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFieldType {
    String,
    StringArray,
    Number,
    Bool,
}

impl<'de> serde::Deserialize<'de> for ConfigFieldType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = <std::string::String as serde::Deserialize>::deserialize(deserializer)?;
        match s.as_str() {
            "string" => Ok(ConfigFieldType::String),
            "string[]" => Ok(ConfigFieldType::StringArray),
            "number" => Ok(ConfigFieldType::Number),
            "bool" => Ok(ConfigFieldType::Bool),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["string", "string[]", "number", "bool"],
            )),
        }
    }
}

/// Schema for a single configuration field declared in plugin.toml
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigFieldSchema {
    #[serde(rename = "type")]
    pub field_type: ConfigFieldType,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub valid_values: Vec<String>,
}

/// A custom command declared by a plugin in plugin.toml
#[derive(Debug, Clone, Deserialize)]
pub struct PluginCommandDef {
    #[serde(default)]
    pub description: String,
    pub handler: String,
}

// ── Host → Plugin messages ──────────────────────────────────────────────

/// Messages sent from the host (lukan) to a plugin process via stdin
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum HostMessage {
    /// Initialize the plugin with its config
    Init {
        name: String,
        config: serde_json::Value,
        #[serde(rename = "protocolVersion")]
        protocol_version: u32,
    },
    /// Response from the agent for a previous channelMessage
    AgentResponse {
        #[serde(rename = "requestId")]
        request_id: String,
        text: String,
        #[serde(rename = "isError")]
        is_error: bool,
    },
    /// Graceful shutdown request
    Shutdown,
}

// ── Plugin → Host messages ──────────────────────────────────────────────

/// Messages sent from a plugin process to the host via stdout
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PluginMessage {
    /// Plugin is ready after receiving Init
    Ready {
        version: String,
        #[serde(default)]
        capabilities: Vec<String>,
    },
    /// Incoming message from the channel (e.g. a WhatsApp/Telegram message)
    ChannelMessage {
        #[serde(rename = "requestId")]
        request_id: String,
        sender: String,
        #[serde(rename = "channelId")]
        channel_id: String,
        content: String,
    },
    /// Plugin status update
    Status { status: PluginStatus },
    /// Log line from the plugin
    Log { level: LogLevel, message: String },
    /// Error from the plugin
    Error { message: String, recoverable: bool },
}

// ── Enums ───────────────────────────────────────────────────────────────

/// Plugin connection status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PluginStatus {
    Connected,
    Disconnected,
    Reconnecting,
    Authenticating,
}

/// Log levels for plugin log messages
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

// ── Plugin manifest (plugin.toml) ───────────────────────────────────────

/// Top-level manifest parsed from plugin.toml
#[derive(Debug, Clone, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    /// How to run the plugin process. Optional for "tools"-type plugins
    /// that only provide tools via tools.json and don't need a long-running process.
    pub run: Option<PluginRunConfig>,
    #[serde(default)]
    pub config: HashMap<String, ConfigFieldSchema>,
    #[serde(default)]
    pub commands: HashMap<String, PluginCommandDef>,
    /// Security policy declared by the plugin
    #[serde(default)]
    pub security: PluginSecurity,
}

/// Security policy declared in plugin.toml `[security]`
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PluginSecurity {
    /// Default tools when none are configured in the plugin's config.json.
    /// Empty = all tools allowed.
    #[serde(default)]
    pub default_tools: Vec<String>,
    /// Include global/project memory in the system prompt
    #[serde(default)]
    pub include_memory: bool,
    /// Enable directory-based path restrictions
    #[serde(default)]
    pub dir_restrictions: bool,
    /// Tool names considered "dangerous" (trigger dir restriction prompts)
    #[serde(default = "default_dangerous_tools")]
    pub dangerous_tools: Vec<String>,
    /// Prompt template filenames for directory restrictions
    #[serde(default)]
    pub prompts: SecurityPrompts,
}

fn default_dangerous_tools() -> Vec<String> {
    vec![
        "Bash".to_string(),
        "WriteFile".to_string(),
        "EditFile".to_string(),
    ]
}

/// Prompt template filenames (relative to plugin dir) for directory restrictions
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SecurityPrompts {
    /// Prompt shown when no directories are allowed
    pub dir_none: Option<String>,
    /// Prompt shown when some directories are allowed (supports `{{ALLOWED_DIRS}}`)
    pub dir_allowed: Option<String>,
}

/// Metadata about the plugin
#[derive(Debug, Clone, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_plugin_type")]
    pub plugin_type: String,
    #[serde(default = "default_protocol_version")]
    pub protocol_version: u32,
    /// CLI alias for this plugin (e.g. "wa" → `lukan wa ...`)
    pub alias: Option<String>,
}

fn default_plugin_type() -> String {
    "channel".to_string()
}

fn default_protocol_version() -> u32 {
    PROTOCOL_VERSION
}

/// How to run the plugin process
#[derive(Debug, Clone, Deserialize)]
pub struct PluginRunConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}
