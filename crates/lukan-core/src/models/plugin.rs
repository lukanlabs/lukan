use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Protocol version for host ↔ plugin communication
pub const PROTOCOL_VERSION: u32 = 1;

// ── Config schema types ──────────────────────────────────────────────

/// Supported configuration field types
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ConfigFieldType {
    #[default]
    String,
    StringArray,
    Number,
    Bool,
}

impl serde::Serialize for ConfigFieldType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            ConfigFieldType::String => serializer.serialize_str("string"),
            ConfigFieldType::StringArray => serializer.serialize_str("string[]"),
            ConfigFieldType::Number => serializer.serialize_str("number"),
            ConfigFieldType::Bool => serializer.serialize_str("bool"),
        }
    }
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

/// Conditional visibility for a config field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependsOn {
    pub field: String,
    pub values: Vec<String>,
}

/// Schema for a single configuration field declared in plugin.toml
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigFieldSchema {
    #[serde(rename = "type")]
    pub field_type: ConfigFieldType,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub valid_values: Vec<String>,
    /// UI label (derived from key if absent)
    #[serde(default)]
    pub label: Option<String>,
    /// Rendering hint: "phone", "url", "password", "textarea"
    #[serde(default)]
    pub format: Option<String>,
    /// UI section grouping (ungrouped → "General")
    #[serde(default)]
    pub group: Option<String>,
    /// Conditional visibility
    #[serde(default)]
    pub depends_on: Option<DependsOn>,
    /// Plugin command that returns JSON options `[{id, label}]`
    #[serde(default)]
    pub options_command: Option<String>,
    /// Hide from UI
    #[serde(default)]
    pub hidden: bool,
    /// Default value
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    /// Sort order within group
    #[serde(default)]
    pub order: i32,
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
    /// Fire-and-forget system event (persisted, injected into agent context)
    SystemEvent {
        source: String,
        level: String,
        detail: String,
    },
    /// Update a declared view's data (persisted to disk, polled by frontend)
    ViewUpdate {
        #[serde(rename = "viewId")]
        view_id: String,
        data: serde_json::Value,
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

// ── Auth declaration ────────────────────────────────────────────────────

/// Authentication method declared in plugin.toml `[auth]`.
/// The host uses this to render the appropriate auth UI without plugin-specific code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthDeclaration {
    /// Plugin writes a QR code to a file; host renders it and polls for completion.
    Qr {
        #[serde(default = "default_qr_file")]
        qr_file: String,
        #[serde(default = "default_status_file")]
        status_file: String,
    },
    /// Authenticated if a config field exists and is non-empty.
    Token {
        #[serde(default = "default_check_field")]
        check_field: String,
    },
    /// Just runs the "auth" command with no special UI.
    Command,
}

fn default_qr_file() -> String {
    "current-qr.txt".to_string()
}

fn default_status_file() -> String {
    "creds.json".to_string()
}

fn default_check_field() -> String {
    "access_token".to_string()
}

// ── Contributions ──────────────────────────────────────────────────────

/// Capabilities a plugin contributes to the host.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginContributions {
    /// Audio transcription service
    pub transcription: Option<TranscriptionContribution>,
}

/// Plugin provides an audio transcription HTTP endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionContribution {
    /// Config key holding the server port (default "port")
    #[serde(default = "default_port_field")]
    pub port_field: String,
    /// Default port if not configured
    #[serde(default = "default_transcription_port")]
    pub default_port: u16,
    /// API endpoint path
    #[serde(default = "default_transcription_endpoint")]
    pub endpoint: String,
}

fn default_port_field() -> String {
    "port".to_string()
}

fn default_transcription_port() -> u16 {
    8787
}

fn default_transcription_endpoint() -> String {
    "/v1/audio/transcriptions".to_string()
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
    /// Authentication method (QR, token check, or command-only)
    pub auth: Option<AuthDeclaration>,
    /// Capabilities this plugin contributes to the host
    #[serde(default)]
    pub contributions: PluginContributions,
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

impl PluginManifest {
    /// Inject config keys implied by `[security]` so they are editable via CLI.
    /// Call this after deserializing the manifest.
    pub fn inject_security_config(&mut self) {
        if self.security.dir_restrictions {
            self.config
                .entry("allowed_dirs".to_string())
                .or_insert(ConfigFieldSchema {
                    field_type: ConfigFieldType::StringArray,
                    description: "Allowed directories for file access".to_string(),
                    valid_values: Vec::new(),
                    hidden: true,
                    ..Default::default()
                });
            self.config
                .entry("skip_dir_restrictions".to_string())
                .or_insert(ConfigFieldSchema {
                    field_type: ConfigFieldType::Bool,
                    description: "Disable directory restrictions".to_string(),
                    valid_values: Vec::new(),
                    hidden: true,
                    ..Default::default()
                });
        }
        if !self.security.default_tools.is_empty() {
            self.config
                .entry("tools".to_string())
                .or_insert(ConfigFieldSchema {
                    field_type: ConfigFieldType::StringArray,
                    description: "Agent tools".to_string(),
                    valid_values: Vec::new(),
                    hidden: true,
                    ..Default::default()
                });
        }
    }
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
    /// Optional activity bar contribution (VS Code-style sidebar icon)
    pub activity_bar: Option<ActivityBarContribution>,
    /// Views declared by the plugin (VS Code-style panels)
    #[serde(default)]
    pub views: Vec<ViewDeclaration>,
}

/// Plugin contribution to the desktop activity bar (like VS Code extensions).
/// Declared in plugin.toml as `[plugin.activity_bar]`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActivityBarContribution {
    /// Lucide icon name (e.g. "container", "shield-alert", "activity")
    pub icon: String,
    /// Label shown on hover
    pub label: String,
}

/// A view declared by a plugin in plugin.toml `[[plugin.views]]`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ViewDeclaration {
    pub id: String,
    #[serde(rename = "type")]
    pub view_type: String,
    pub label: String,
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
    /// Handler script filename (e.g. "tools.py"). Defaults to "tools.js" if absent.
    pub handler: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ConfigFieldType ─────────────────────────────────────────────

    #[test]
    fn test_config_field_type_serde() {
        assert_eq!(
            serde_json::to_string(&ConfigFieldType::String).unwrap(),
            r#""string""#
        );
        assert_eq!(
            serde_json::to_string(&ConfigFieldType::StringArray).unwrap(),
            r#""string[]""#
        );
        assert_eq!(
            serde_json::to_string(&ConfigFieldType::Number).unwrap(),
            r#""number""#
        );
        assert_eq!(
            serde_json::to_string(&ConfigFieldType::Bool).unwrap(),
            r#""bool""#
        );

        let parsed: ConfigFieldType = serde_json::from_str(r#""string[]""#).unwrap();
        assert_eq!(parsed, ConfigFieldType::StringArray);
    }

    #[test]
    fn test_config_field_type_default() {
        let ft = ConfigFieldType::default();
        assert_eq!(ft, ConfigFieldType::String);
    }

    #[test]
    fn test_config_field_type_invalid() {
        let result = serde_json::from_str::<ConfigFieldType>(r#""invalid""#);
        assert!(result.is_err());
    }

    // ── PluginStatus ────────────────────────────────────────────────

    #[test]
    fn test_plugin_status_serde() {
        assert_eq!(
            serde_json::to_string(&PluginStatus::Connected).unwrap(),
            r#""connected""#
        );
        assert_eq!(
            serde_json::to_string(&PluginStatus::Reconnecting).unwrap(),
            r#""reconnecting""#
        );
        assert_eq!(
            serde_json::to_string(&PluginStatus::Authenticating).unwrap(),
            r#""authenticating""#
        );

        let parsed: PluginStatus = serde_json::from_str(r#""disconnected""#).unwrap();
        assert_eq!(parsed, PluginStatus::Disconnected);
    }

    // ── LogLevel ────────────────────────────────────────────────────

    #[test]
    fn test_log_level_serde() {
        assert_eq!(
            serde_json::to_string(&LogLevel::Debug).unwrap(),
            r#""debug""#
        );
        assert_eq!(serde_json::to_string(&LogLevel::Info).unwrap(), r#""info""#);
        assert_eq!(serde_json::to_string(&LogLevel::Warn).unwrap(), r#""warn""#);
        assert_eq!(
            serde_json::to_string(&LogLevel::Error).unwrap(),
            r#""error""#
        );
    }

    // ── HostMessage ─────────────────────────────────────────────────

    #[test]
    fn test_host_message_init_serde() {
        let msg = HostMessage::Init {
            name: "whatsapp".into(),
            config: serde_json::json!({"phone": "+123"}),
            protocol_version: 1,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"init""#));
        assert!(json.contains(r#""protocolVersion":1"#));

        let parsed: HostMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            HostMessage::Init {
                name,
                protocol_version,
                ..
            } => {
                assert_eq!(name, "whatsapp");
                assert_eq!(protocol_version, 1);
            }
            _ => panic!("Expected Init"),
        }
    }

    #[test]
    fn test_host_message_agent_response_serde() {
        let msg = HostMessage::AgentResponse {
            request_id: "r1".into(),
            text: "response text".into(),
            is_error: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"agentResponse""#));
        assert!(json.contains(r#""requestId":"r1""#));
        assert!(json.contains(r#""isError":false"#));
    }

    #[test]
    fn test_host_message_shutdown_serde() {
        let msg = HostMessage::Shutdown;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"shutdown""#));
    }

    // ── PluginMessage ───────────────────────────────────────────────

    #[test]
    fn test_plugin_message_ready_serde() {
        let msg = PluginMessage::Ready {
            version: "1.0.0".into(),
            capabilities: vec!["voice".into()],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"ready""#));

        let parsed: PluginMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            PluginMessage::Ready {
                version,
                capabilities,
            } => {
                assert_eq!(version, "1.0.0");
                assert_eq!(capabilities, vec!["voice"]);
            }
            _ => panic!("Expected Ready"),
        }
    }

    #[test]
    fn test_plugin_message_channel_message_serde() {
        let msg = PluginMessage::ChannelMessage {
            request_id: "r1".into(),
            sender: "John".into(),
            channel_id: "ch-1".into(),
            content: "hello there".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""requestId":"r1""#));
        assert!(json.contains(r#""channelId":"ch-1""#));
    }

    #[test]
    fn test_plugin_message_error_serde() {
        let msg = PluginMessage::Error {
            message: "crash".into(),
            recoverable: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#""recoverable":false"#));
    }

    #[test]
    fn test_plugin_message_log_serde() {
        let msg = PluginMessage::Log {
            level: LogLevel::Warn,
            message: "something odd".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""level":"warn""#));
    }

    // ── AuthDeclaration ─────────────────────────────────────────────

    #[test]
    fn test_auth_declaration_qr_defaults() {
        let json = r#"{"type":"qr"}"#;
        let auth: AuthDeclaration = serde_json::from_str(json).unwrap();
        match auth {
            AuthDeclaration::Qr {
                qr_file,
                status_file,
            } => {
                assert_eq!(qr_file, "current-qr.txt");
                assert_eq!(status_file, "creds.json");
            }
            _ => panic!("Expected Qr"),
        }
    }

    #[test]
    fn test_auth_declaration_token_default() {
        let json = r#"{"type":"token"}"#;
        let auth: AuthDeclaration = serde_json::from_str(json).unwrap();
        match auth {
            AuthDeclaration::Token { check_field } => {
                assert_eq!(check_field, "access_token");
            }
            _ => panic!("Expected Token"),
        }
    }

    #[test]
    fn test_auth_declaration_command() {
        let json = r#"{"type":"command"}"#;
        let auth: AuthDeclaration = serde_json::from_str(json).unwrap();
        assert!(matches!(auth, AuthDeclaration::Command));
    }

    // ── PluginManifest ──────────────────────────────────────────────

    #[test]
    fn test_plugin_manifest_toml_parse() {
        let toml_str = r#"
            [plugin]
            name = "test-plugin"
            version = "0.1.0"
            description = "A test plugin"

            [run]
            command = "node"
            args = ["index.js"]

            [config.api_key]
            type = "string"
            description = "API key"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.plugin.name, "test-plugin");
        assert_eq!(manifest.plugin.version, "0.1.0");
        assert!(manifest.run.is_some());
        let run = manifest.run.unwrap();
        assert_eq!(run.command, "node");
        assert_eq!(run.args, vec!["index.js"]);
        assert!(manifest.config.contains_key("api_key"));
    }

    #[test]
    fn test_plugin_manifest_defaults() {
        let toml_str = r#"
            [plugin]
            name = "minimal"
            version = "0.1.0"
        "#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.plugin.plugin_type, "channel");
        assert_eq!(manifest.plugin.protocol_version, PROTOCOL_VERSION);
        assert!(manifest.run.is_none());
        assert!(manifest.config.is_empty());
        assert!(manifest.commands.is_empty());
        assert!(manifest.auth.is_none());
    }

    #[test]
    fn test_inject_security_config_dir_restrictions() {
        let toml_str = r#"
            [plugin]
            name = "secured"
            version = "0.1.0"

            [security]
            dir_restrictions = true
            default_tools = ["Bash", "ReadFiles"]
        "#;
        let mut manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert!(!manifest.config.contains_key("allowed_dirs"));
        assert!(!manifest.config.contains_key("tools"));

        manifest.inject_security_config();

        assert!(manifest.config.contains_key("allowed_dirs"));
        assert_eq!(
            manifest.config["allowed_dirs"].field_type,
            ConfigFieldType::StringArray
        );
        assert!(manifest.config.contains_key("skip_dir_restrictions"));
        assert!(manifest.config.contains_key("tools"));
    }

    #[test]
    fn test_inject_security_config_no_dir_restrictions() {
        let toml_str = r#"
            [plugin]
            name = "open"
            version = "0.1.0"
        "#;
        let mut manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        manifest.inject_security_config();
        // Should not inject anything since dir_restrictions is false
        assert!(!manifest.config.contains_key("allowed_dirs"));
        assert!(!manifest.config.contains_key("tools"));
    }

    // ── TranscriptionContribution ───────────────────────────────────

    #[test]
    fn test_transcription_contribution_defaults() {
        let json = r#"{}"#;
        let tc: TranscriptionContribution = serde_json::from_str(json).unwrap();
        assert_eq!(tc.port_field, "port");
        assert_eq!(tc.default_port, 8787);
        assert_eq!(tc.endpoint, "/v1/audio/transcriptions");
    }

    // ── PROTOCOL_VERSION ────────────────────────────────────────────

    #[test]
    fn test_protocol_version_is_1() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }
}
