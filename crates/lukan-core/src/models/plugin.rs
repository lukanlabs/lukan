use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Protocol version for host ↔ plugin communication
pub const PROTOCOL_VERSION: u32 = 1;

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
    Log {
        level: LogLevel,
        message: String,
    },
    /// Error from the plugin
    Error {
        message: String,
        recoverable: bool,
    },
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
    pub run: PluginRunConfig,
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
