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
    /// Bind daemon to localhost only (not accessible from the network)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub local_only: bool,
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
            local_only: false,
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── ProviderName ────────────────────────────────────────────────

    #[test]
    fn test_provider_name_serde_kebab_case() {
        let name = ProviderName::GithubCopilot;
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, r#""github-copilot""#);

        let parsed: ProviderName = serde_json::from_str(r#""github-copilot""#).unwrap();
        assert_eq!(parsed, ProviderName::GithubCopilot);
    }

    #[test]
    fn test_provider_name_all_variants_roundtrip() {
        let variants = [
            ProviderName::Nebius,
            ProviderName::Anthropic,
            ProviderName::Fireworks,
            ProviderName::GithubCopilot,
            ProviderName::OpenaiCodex,
            ProviderName::Zai,
            ProviderName::OllamaCloud,
            ProviderName::OpenaiCompatible,
            ProviderName::LukanCloud,
            ProviderName::Gemini,
        ];
        for variant in &variants {
            let json = serde_json::to_string(variant).unwrap();
            let parsed: ProviderName = serde_json::from_str(&json).unwrap();
            assert_eq!(&parsed, variant);
        }
    }

    #[test]
    fn test_provider_name_display() {
        assert_eq!(ProviderName::Nebius.to_string(), "nebius");
        assert_eq!(ProviderName::Anthropic.to_string(), "anthropic");
        assert_eq!(ProviderName::GithubCopilot.to_string(), "github-copilot");
        assert_eq!(ProviderName::OpenaiCodex.to_string(), "openai-codex");
        assert_eq!(ProviderName::OllamaCloud.to_string(), "ollama-cloud");
        assert_eq!(
            ProviderName::OpenaiCompatible.to_string(),
            "openai-compatible"
        );
        assert_eq!(ProviderName::LukanCloud.to_string(), "lukan-cloud");
        assert_eq!(ProviderName::Gemini.to_string(), "gemini");
    }

    #[test]
    fn test_provider_name_hash_eq() {
        let mut map = HashMap::new();
        map.insert(ProviderName::Anthropic, "key1");
        map.insert(ProviderName::Nebius, "key2");
        assert_eq!(map.get(&ProviderName::Anthropic), Some(&"key1"));
        assert_eq!(map.get(&ProviderName::Nebius), Some(&"key2"));
        assert_eq!(map.get(&ProviderName::Gemini), None);
    }

    // ── AppConfig ───────────────────────────────────────────────────

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert_eq!(config.provider, ProviderName::Nebius);
        assert_eq!(config.max_tokens, 8192);
        assert!(config.model.is_none());
        assert!(config.temperature.is_none());
        assert!(config.plugins.is_none());
        assert!(config.mcp_servers.is_empty());
    }

    #[test]
    fn test_app_config_serde_roundtrip() {
        let config = AppConfig {
            provider: ProviderName::Anthropic,
            model: Some("claude-3".into()),
            max_tokens: 4096,
            temperature: Some(0.7),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""provider":"anthropic""#));
        assert!(json.contains(r#""model":"claude-3""#));
        assert!(json.contains(r#""maxTokens":4096"#));

        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.provider, ProviderName::Anthropic);
        assert_eq!(parsed.model.as_deref(), Some("claude-3"));
        assert_eq!(parsed.max_tokens, 4096);
    }

    #[test]
    fn test_app_config_max_tokens_default_on_missing() {
        let json = r#"{"provider":"nebius"}"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_tokens, 8192);
    }

    #[test]
    fn test_app_config_skip_serializing_none_fields() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        // Optional None fields should be omitted
        assert!(!json.contains("model"));
        assert!(!json.contains("temperature"));
        assert!(!json.contains("webPassword"));
    }

    #[test]
    fn test_app_config_mcp_servers() {
        let json = r#"{
            "provider": "nebius",
            "mcpServers": {
                "test-server": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": {"PORT": "3000"}
                }
            }
        }"#;
        let config: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.mcp_servers.len(), 1);
        let server = &config.mcp_servers["test-server"];
        assert_eq!(server.command, "node");
        assert_eq!(server.args, vec!["server.js"]);
        assert_eq!(server.env.get("PORT").unwrap(), "3000");
    }

    // ── Credentials ─────────────────────────────────────────────────

    #[test]
    fn test_credentials_default() {
        let creds = Credentials::default();
        assert!(creds.anthropic_api_key.is_none());
        assert!(creds.nebius_api_key.is_none());
        assert!(creds.skill_credentials.is_empty());
    }

    #[test]
    fn test_credentials_serde_roundtrip() {
        let creds = Credentials {
            anthropic_api_key: Some("sk-ant-123".into()),
            nebius_api_key: Some("neb-456".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&creds).unwrap();
        assert!(json.contains(r#""anthropicApiKey":"sk-ant-123""#));

        let parsed: Credentials = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.anthropic_api_key.as_deref(), Some("sk-ant-123"));
        assert_eq!(parsed.nebius_api_key.as_deref(), Some("neb-456"));
    }

    #[test]
    fn test_credentials_flatten_skill_env() {
        let mut skill_creds = HashMap::new();
        let mut skill_a = HashMap::new();
        skill_a.insert("API_KEY".to_string(), "key_a".to_string());
        skill_a.insert("SECRET".to_string(), "sec_a".to_string());
        skill_creds.insert("skill-a".to_string(), skill_a);

        let mut skill_b = HashMap::new();
        skill_b.insert("OTHER_KEY".to_string(), "key_b".to_string());
        skill_creds.insert("skill-b".to_string(), skill_b);

        let creds = Credentials {
            skill_credentials: skill_creds,
            ..Default::default()
        };

        let flat = creds.flatten_skill_env();
        assert_eq!(flat.get("API_KEY").unwrap(), "key_a");
        assert_eq!(flat.get("SECRET").unwrap(), "sec_a");
        assert_eq!(flat.get("OTHER_KEY").unwrap(), "key_b");
    }

    #[test]
    fn test_credentials_flatten_skill_env_empty() {
        let creds = Credentials::default();
        assert!(creds.flatten_skill_env().is_empty());
    }

    // ── ResolvedConfig ──────────────────────────────────────────────

    #[test]
    fn test_effective_model_none_when_unset() {
        let rc = ResolvedConfig {
            config: AppConfig::default(),
            credentials: Credentials::default(),
        };
        assert!(rc.effective_model().is_none());
    }

    #[test]
    fn test_effective_model_plain() {
        let rc = ResolvedConfig {
            config: AppConfig {
                model: Some("claude-3-opus".into()),
                ..Default::default()
            },
            credentials: Credentials::default(),
        };
        assert_eq!(rc.effective_model().as_deref(), Some("claude-3-opus"));
    }

    #[test]
    fn test_effective_model_strips_provider_prefix() {
        let rc = ResolvedConfig {
            config: AppConfig {
                provider: ProviderName::Anthropic,
                model: Some("anthropic:claude-3-opus".into()),
                ..Default::default()
            },
            credentials: Credentials::default(),
        };
        assert_eq!(rc.effective_model().as_deref(), Some("claude-3-opus"));
    }

    #[test]
    fn test_effective_model_does_not_strip_other_provider_prefix() {
        let rc = ResolvedConfig {
            config: AppConfig {
                provider: ProviderName::Nebius,
                model: Some("anthropic:claude-3-opus".into()),
                ..Default::default()
            },
            credentials: Credentials::default(),
        };
        // Should not strip because current provider is Nebius, not Anthropic
        assert_eq!(
            rc.effective_model().as_deref(),
            Some("anthropic:claude-3-opus")
        );
    }

    // ── PermissionMode ──────────────────────────────────────────────

    #[test]
    fn test_permission_mode_default() {
        let mode = PermissionMode::default();
        assert_eq!(mode, PermissionMode::Auto);
    }

    #[test]
    fn test_permission_mode_display() {
        assert_eq!(PermissionMode::Manual.to_string(), "manual");
        assert_eq!(PermissionMode::Auto.to_string(), "auto");
        assert_eq!(PermissionMode::Skip.to_string(), "skip");
        assert_eq!(PermissionMode::Planner.to_string(), "planner");
    }

    #[test]
    fn test_permission_mode_next_cycles() {
        let start = PermissionMode::Manual;
        let step1 = start.next();
        assert_eq!(step1, PermissionMode::Auto);
        let step2 = step1.next();
        assert_eq!(step2, PermissionMode::Skip);
        let step3 = step2.next();
        assert_eq!(step3, PermissionMode::Planner);
        let step4 = step3.next();
        assert_eq!(step4, PermissionMode::Manual);
    }

    #[test]
    fn test_permission_mode_serde() {
        let mode = PermissionMode::Planner;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, r#""planner""#);

        let parsed: PermissionMode = serde_json::from_str(r#""auto""#).unwrap();
        assert_eq!(parsed, PermissionMode::Auto);
    }

    // ── PermissionsConfig ───────────────────────────────────────────

    #[test]
    fn test_permissions_config_default() {
        let perms = PermissionsConfig::default();
        assert!(perms.deny.is_empty());
        assert!(perms.ask.is_empty());
        assert!(perms.allow.is_empty());
        assert!(perms.os_sandbox);
        assert!(!perms.sensitive_patterns.is_empty());
        // Spot-check some default patterns
        assert!(perms.sensitive_patterns.contains(&".env*".to_string()));
        assert!(perms.sensitive_patterns.contains(&".ssh/".to_string()));
        assert!(perms.sensitive_patterns.contains(&"*.pem".to_string()));
    }

    #[test]
    fn test_permissions_config_serde_roundtrip() {
        let perms = PermissionsConfig {
            deny: vec!["rm -rf".into()],
            ask: vec!["Bash".into()],
            allow: vec!["ReadFiles".into()],
            os_sandbox: false,
            sensitive_patterns: vec![".secret".into()],
        };
        let json = serde_json::to_string(&perms).unwrap();
        let parsed: PermissionsConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.deny, vec!["rm -rf"]);
        assert!(!parsed.os_sandbox);
        assert_eq!(parsed.sensitive_patterns, vec![".secret"]);
    }

    // ── PluginsConfig ───────────────────────────────────────────────

    #[test]
    fn test_plugins_config_default() {
        let pc = PluginsConfig::default();
        assert!(pc.enabled.is_empty());
        assert!(pc.overrides.is_empty());
    }

    #[test]
    fn test_plugins_config_serde() {
        let json = r#"{"enabled":["whatsapp"],"overrides":{"whatsapp":{"provider":"anthropic","model":"claude-3"}}}"#;
        let pc: PluginsConfig = serde_json::from_str(json).unwrap();
        assert_eq!(pc.enabled, vec!["whatsapp"]);
        let ov = &pc.overrides["whatsapp"];
        assert_eq!(ov.provider, Some(ProviderName::Anthropic));
        assert_eq!(ov.model.as_deref(), Some("claude-3"));
    }

    // ── McpServerConfig ─────────────────────────────────────────────

    #[test]
    fn test_mcp_server_config_defaults() {
        let json = r#"{"command":"npx"}"#;
        let cfg: McpServerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.command, "npx");
        assert!(cfg.args.is_empty());
        assert!(cfg.env.is_empty());
    }

    // ── TOOL_GROUPS constant ────────────────────────────────────────

    #[test]
    fn test_tool_groups_not_empty() {
        assert!(!TOOL_GROUPS.is_empty());
        for (group_name, tools) in TOOL_GROUPS {
            assert!(!group_name.is_empty());
            assert!(!tools.is_empty());
        }
    }

    #[test]
    fn test_tool_groups_contains_expected() {
        let names: Vec<&str> = TOOL_GROUPS.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"File ops"));
        assert!(names.contains(&"Search"));
        assert!(names.contains(&"Execution"));
        assert!(names.contains(&"Browser"));
    }
}
