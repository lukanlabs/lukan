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
    OllamaCloud,
    OpenaiCompatible,
    LukanCloud,
    Gemini,
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
            ProviderName::OllamaCloud => write!(f, "ollama-cloud"),
            ProviderName::OpenaiCompatible => write!(f, "openai-compatible"),
            ProviderName::LukanCloud => write!(f, "lukan-cloud"),
            ProviderName::Gemini => write!(f, "gemini"),
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
    /// CDP URL for browser tools (overrides auto-launch)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_cdp_url: Option<String>,
    /// Tools to disable (by name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_tools: Option<Vec<String>>,
    /// MCP (Model Context Protocol) server configurations
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

/// Configuration for an MCP (Model Context Protocol) server.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
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
            openai_compatible_base_url: None,
            openai_compatible_provider_name: None,
            openai_compatible_provider_options: None,
            web_password: None,
            web_token_ttl: None,
            plugins: None,
            browser_cdp_url: None,
            disabled_tools: None,
            mcp_servers: HashMap::new(),
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
    pub ollama_cloud_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_compatible_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lukan_cloud_api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini_api_key: Option<String>,
    /// Per-skill environment variables (e.g. {"nano-banana-pro": {"GEMINI_API_KEY": "..."}})
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub skill_credentials: HashMap<String, HashMap<String, String>>,
}

impl Credentials {
    /// Flatten all per-skill env vars into a single map for injection into Bash.
    /// If two skills define the same var name the last one (alphabetically) wins.
    pub fn flatten_skill_env(&self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        for vars in self.skill_credentials.values() {
            out.extend(vars.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        out
    }
}

/// Config + credentials resolved together
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub config: AppConfig,
    pub credentials: Credentials,
}

impl ResolvedConfig {
    /// Get the effective model, if one is configured.
    /// Returns `None` when no model has been selected (user must pick one via `/model`).
    /// Strips "provider:" prefix if present (models list stores entries as
    /// "provider:model_id" and legacy configs may have saved the prefixed form).
    pub fn effective_model(&self) -> Option<String> {
        let raw = self.config.model.clone()?;
        let prefix = format!("{}:", self.config.provider);
        if raw.starts_with(&prefix) {
            Some(raw[prefix.len()..].to_string())
        } else {
            Some(raw)
        }
    }
}

/// Tool groups for categorized display
pub const TOOL_GROUPS: &[(&str, &[&str])] = &[
    ("File ops", &["ReadFiles", "WriteFile", "EditFile"]),
    ("Search", &["Grep", "Glob"]),
    ("Agent", &["Explore", "SubAgent", "SubAgentResult"]),
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
    (
        "Browser",
        &[
            "BrowserNavigate",
            "BrowserSnapshot",
            "BrowserScreenshot",
            "BrowserClick",
            "BrowserType",
            "BrowserEvaluate",
            "BrowserTabs",
            "BrowserNewTab",
            "BrowserSwitchTab",
            "BrowserSavePDF",
        ],
    ),
];

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

impl fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PermissionMode::Manual => write!(f, "manual"),
            PermissionMode::Auto => write!(f, "auto"),
            PermissionMode::Skip => write!(f, "skip"),
            PermissionMode::Planner => write!(f, "planner"),
        }
    }
}

impl PermissionMode {
    /// Cycle to next mode: manual → auto → skip → planner → manual
    pub fn next(&self) -> Self {
        match self {
            PermissionMode::Manual => PermissionMode::Auto,
            PermissionMode::Auto => PermissionMode::Skip,
            PermissionMode::Skip => PermissionMode::Planner,
            PermissionMode::Planner => PermissionMode::Manual,
        }
    }
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
    /// Gitignore-style patterns to block sensitive files/directories.
    /// Patterns ending with `/` block entire directories (e.g. `.ssh/`).
    /// Other patterns match against filenames (e.g. `*.pem`, `.env*`).
    #[serde(default = "default_sensitive_patterns")]
    pub sensitive_patterns: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_sensitive_patterns() -> Vec<String> {
    vec![
        // File patterns
        ".env*".into(),
        "credentials.json".into(),
        "*.pem".into(),
        "*.key".into(),
        "*.p12".into(),
        "*.pfx".into(),
        ".npmrc".into(),
        ".netrc".into(),
        // Directory patterns
        ".ssh/".into(),
        ".gnupg/".into(),
        ".aws/".into(),
        ".docker/".into(),
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
