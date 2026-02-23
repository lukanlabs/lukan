use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// LLM provider identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderName {
    Nebius,
    Anthropic,
    Fireworks,
    GithubCopilot,
    OpenaiCodex,
    Zai,
    OpenaiCompatible,
}

impl fmt::Display for ProviderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderName::Nebius => write!(f, "nebius"),
            ProviderName::Anthropic => write!(f, "anthropic"),
            ProviderName::Fireworks => write!(f, "fireworks"),
            ProviderName::GithubCopilot => write!(f, "github-copilot"),
            ProviderName::OpenaiCodex => write!(f, "openai-codex"),
            ProviderName::Zai => write!(f, "zai"),
            ProviderName::OpenaiCompatible => write!(f, "openai-compatible"),
        }
    }
}

impl ProviderName {
    /// Default model for each provider
    pub fn default_model(&self) -> &'static str {
        match self {
            ProviderName::Nebius => "MiniMaxAI/MiniMax-M2.1",
            ProviderName::Anthropic => "claude-sonnet-4-5-20250929",
            ProviderName::Fireworks => "accounts/fireworks/models/minimax-m2p5",
            ProviderName::GithubCopilot => "claude-sonnet-4.5",
            ProviderName::OpenaiCodex => "gpt-5.3-codex",
            ProviderName::Zai => "glm-5",
            ProviderName::OpenaiCompatible => "default",
        }
    }
}

/// Main application configuration (persisted as config.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub provider: ProviderName,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub syntax_theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision_models: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whatsapp: Option<WhatsAppConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<EmailConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_compatible_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_compatible_provider_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_compatible_provider_options: Option<HashMap<String, serde_json::Value>>,
    /// Password for web UI authentication (None = no auth required)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_password: Option<String>,
    /// Web auth token TTL in hours (default: 24)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_token_ttl: Option<u64>,
    /// Plugin system configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugins: Option<PluginsConfig>,
}

fn default_max_tokens() -> u32 {
    8192
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: ProviderName::Nebius,
            model: None,
            max_tokens: default_max_tokens(),
            temperature: None,
            timezone: None,
            syntax_theme: None,
            models: None,
            vision_model: None,
            vision_models: None,
            whatsapp: None,
            email: None,
            openai_compatible_base_url: None,
            openai_compatible_provider_name: None,
            openai_compatible_provider_options: None,
            web_password: None,
            web_token_ttl: None,
            plugins: None,
        }
    }
}

/// API credentials (persisted as credentials.json)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nebius_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fireworks_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copilot_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copilot_client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brave_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tavily_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_token_expiry: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zai_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_compatible_api_key: Option<String>,
}

/// Config + credentials resolved together
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub config: AppConfig,
    pub credentials: Credentials,
}

impl ResolvedConfig {
    /// Get the effective model (config override or provider default)
    pub fn effective_model(&self) -> String {
        self.config
            .model
            .clone()
            .unwrap_or_else(|| self.config.provider.default_model().to_string())
    }
}

/// Default tools enabled for WhatsApp channel
pub const WA_DEFAULT_TOOLS: &[&str] = &["Grep", "Glob", "ReadFile", "WebFetch"];

/// All tools available for WhatsApp channel
pub const WA_ALL_TOOLS: &[&str] = &[
    "ReadFile",
    "WriteFile",
    "EditFile",
    "Grep",
    "Glob",
    "Bash",
    "WebFetch",
    "SheetsRead",
    "SheetsWrite",
    "SheetsCreate",
    "CalendarList",
    "CalendarCreate",
    "CalendarUpdate",
    "DocsRead",
    "DocsCreate",
    "DocsUpdate",
    "DriveList",
    "DriveDownload",
];

/// Tool groups for categorized display
pub const TOOL_GROUPS: &[(&str, &[&str])] = &[
    ("File ops", &["ReadFile", "WriteFile", "EditFile"]),
    ("Search", &["Grep", "Glob"]),
    ("Execution", &["Bash"]),
    ("Web", &["WebFetch"]),
    (
        "Google Sheets",
        &["SheetsRead", "SheetsWrite", "SheetsCreate"],
    ),
    (
        "Google Calendar",
        &["CalendarList", "CalendarCreate", "CalendarUpdate"],
    ),
    ("Google Docs", &["DocsRead", "DocsCreate", "DocsUpdate"]),
    ("Google Drive", &["DriveList", "DriveDownload"]),
];

/// WhatsApp channel configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppConfig {
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
    pub provider: Option<ProviderName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder_advance: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reminder_chat: Option<String>,
}

/// Email channel configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smtp: Option<EmailSmtpConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imap: Option<EmailImapConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whitelist: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailSmtpConfig {
    pub host: String,
    pub port: u16,
    pub secure: bool,
    pub user: String,
    pub pass: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailImapConfig {
    pub host: String,
    pub port: u16,
    pub secure: bool,
    pub user: String,
    pub pass: String,
}

/// Permission mode for tool execution
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    Manual,
    #[default]
    Auto,
    Skip,
    Planner,
}

/// Permission rules configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsConfig {
    #[serde(default)]
    pub deny: Vec<String>,
    #[serde(default)]
    pub ask: Vec<String>,
    #[serde(default)]
    pub allow: Vec<String>,
    /// Enable OS-level sandbox (bwrap) -- default true
    #[serde(default = "default_true")]
    pub os_sandbox: bool,
    /// File patterns to block inside sandbox
    #[serde(default = "default_sensitive_patterns")]
    pub sensitive_patterns: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_sensitive_patterns() -> Vec<String> {
    vec![
        ".env*".into(),
        "credentials.json".into(),
        "*.pem".into(),
        "*.key".into(),
        "*.p12".into(),
        ".npmrc".into(),
    ]
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            deny: Vec::new(),
            ask: Vec::new(),
            allow: Vec::new(),
            os_sandbox: default_true(),
            sensitive_patterns: default_sensitive_patterns(),
        }
    }
}

/// Plugin system configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginsConfig {
    /// List of plugin names to auto-start
    #[serde(default)]
    pub enabled: Vec<String>,
    /// Per-plugin overrides
    #[serde(default)]
    pub overrides: HashMap<String, PluginOverrides>,
}

/// Per-plugin configuration overrides
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderName>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_response_len: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_restart: Option<bool>,
}
