mod bash;
pub mod bg_processes;
pub mod browser;
mod edit_file;
mod glob_tool;
mod grep;
pub mod mcp;
pub mod mcp_tools;
pub mod planner_tools;
pub mod plugin_tools;
mod read_file;
pub mod sandbox;
pub mod skills;
pub mod tasks;
mod web_fetch;
mod write_file;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use lukan_core::models::events::StreamEvent;
use lukan_core::models::tools::{ToolDefinition, ToolResult};
use tokio::sync::{Mutex, mpsc, watch};
use tokio_util::sync::CancellationToken;

/// Context passed to every tool execution
pub struct ToolContext {
    /// Optional channel for progress updates
    pub progress_tx: Option<mpsc::Sender<String>>,
    /// Optional channel to emit StreamEvent to the TUI (used by Explore tool)
    pub event_tx: Option<mpsc::Sender<StreamEvent>>,
    /// The tool_use ID assigned by the LLM for this call (used by Explore for progress)
    pub tool_call_id: Option<String>,
    /// Tracks which files have been read (for write/edit guards)
    pub read_files: Arc<Mutex<HashSet<PathBuf>>>,
    /// Current working directory
    pub cwd: PathBuf,
    /// Signal to send a running Bash command to background (Alt+B)
    pub bg_signal: Option<watch::Receiver<()>>,
    /// OS-level sandbox configuration (bwrap)
    pub sandbox: Option<sandbox::SandboxConfig>,
    /// Hard path restrictions — when set, only paths under these dirs are allowed
    pub allowed_paths: Option<Vec<PathBuf>>,
    /// Cancellation token — when cancelled, long-running tools should abort
    pub cancel: Option<CancellationToken>,
    /// Session ID for associating background processes with the chat session
    pub session_id: Option<String>,
    /// Extra environment variables injected into Bash commands (e.g. skill credentials)
    pub extra_env: HashMap<String, String>,
    /// Human-readable label for the agent/tab that owns this context (e.g. "Agent 2")
    pub agent_label: Option<String>,
    /// Tab ID for associating background processes with the frontend tab
    pub tab_id: Option<String>,
}

impl ToolContext {
    /// Check if a path is allowed under the configured restrictions.
    /// Returns `Ok(())` if allowed, or an error `ToolResult` if blocked.
    pub fn check_path_allowed(&self, path: &std::path::Path) -> Result<(), String> {
        let dirs = match &self.allowed_paths {
            Some(dirs) if !dirs.is_empty() => dirs,
            _ => return Ok(()),
        };
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        for allowed in dirs {
            let allowed_canon = allowed
                .canonicalize()
                .unwrap_or_else(|_| allowed.to_path_buf());
            if canonical.starts_with(&allowed_canon) {
                return Ok(());
            }
        }
        Err(format!(
            "Path '{}' is outside allowed directories. Allowed: {}",
            path.display(),
            dirs.iter()
                .map(|d| d.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }

    /// Check if a path points to a sensitive file or is inside a sensitive directory.
    ///
    /// Supports gitignore-style patterns:
    /// - `*.pem`, `.env*` — match against the filename
    /// - `.ssh/`, `.aws/` — match against any path component (blocks the entire subtree)
    ///
    /// Uses `sensitive_patterns` from the sandbox config when available,
    /// otherwise falls back to `DEFAULT_SENSITIVE_PATTERNS`.
    /// Checks both the original path and the canonicalized path to prevent symlink bypass.
    pub fn check_sensitive(&self, path: &std::path::Path) -> Result<(), String> {
        let patterns: Vec<&str> = if let Some(ref sb) = self.sandbox {
            if sb.sensitive_patterns.is_empty() {
                sandbox::DEFAULT_SENSITIVE_PATTERNS.to_vec()
            } else {
                sb.sensitive_patterns.iter().map(|s| s.as_str()).collect()
            }
        } else {
            sandbox::DEFAULT_SENSITIVE_PATTERNS.to_vec()
        };

        // Check both original path and canonicalized path (symlink bypass prevention)
        if let Some(matched) = sandbox::match_sensitive_pattern(path, &patterns) {
            return Err(format!(
                "Access denied: '{}' matches sensitive pattern '{}'",
                path.display(),
                matched
            ));
        }

        if let Ok(canonical) = path.canonicalize()
            && canonical != path
            && let Some(matched) = sandbox::match_sensitive_pattern(&canonical, &patterns)
        {
            return Err(format!(
                "Access denied: '{}' matches sensitive pattern '{}'",
                path.display(),
                matched
            ));
        }

        Ok(())
    }
}

/// Trait that all tools must implement
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g. "Bash", "ReadFiles")
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// JSON Schema for the tool's input
    fn input_schema(&self) -> serde_json::Value;

    /// Source plugin name, if this tool comes from a plugin
    fn source(&self) -> Option<&str> {
        None
    }

    /// Execute the tool with parsed JSON input
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult>;
}

/// Registry that holds all available tools
pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
    /// Whether the OS-level sandbox (bwrap) is enabled
    sandbox_enabled: bool,
    /// Directories allowed to be writable inside the sandbox
    allowed_dirs: Vec<String>,
    /// File patterns to block inside the sandbox
    sensitive_patterns: Vec<String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            sandbox_enabled: false,
            allowed_dirs: Vec::new(),
            sensitive_patterns: Vec::new(),
        }
    }

    /// Register a tool
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    /// Execute a tool by name
    pub async fn execute(
        &self,
        name: &str,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        match self.get(name) {
            Some(tool) => tool.execute(input, ctx).await,
            None => Ok(ToolResult::error(format!("Unknown tool: {name}"))),
        }
    }

    /// Get tool definitions for the LLM
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        let mut defs: Vec<_> = self
            .tools
            .values()
            .map(|t| ToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
            })
            .collect();
        defs.sort_by(|a, b| a.name.cmp(&b.name));
        defs
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    /// Retain only tools whose names are in the allowed list
    pub fn retain(&mut self, allowed: &[&str]) {
        self.tools
            .retain(|name, _| allowed.contains(&name.as_str()));
    }

    /// Configure the OS-level sandbox (bwrap) settings
    pub fn set_sandbox(
        &mut self,
        enabled: bool,
        allowed_dirs: Vec<String>,
        sensitive_patterns: Vec<String>,
    ) {
        self.sandbox_enabled = enabled;
        self.allowed_dirs = allowed_dirs;
        self.sensitive_patterns = sensitive_patterns;
    }

    /// Check if the OS-level sandbox is enabled
    pub fn is_sandbox_enabled(&self) -> bool {
        self.sandbox_enabled
    }

    /// Get the allowed writable directories for the sandbox
    pub fn allowed_dirs(&self) -> &[String] {
        &self.allowed_dirs
    }

    /// Get the sensitive file patterns for the sandbox
    pub fn sensitive_patterns(&self) -> &[String] {
        &self.sensitive_patterns
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_ctx(allowed: Option<Vec<&str>>) -> ToolContext {
        ToolContext {
            progress_tx: None,
            event_tx: None,
            tool_call_id: None,
            read_files: Arc::new(Mutex::new(HashSet::new())),
            cwd: PathBuf::from("/tmp"),
            bg_signal: None,
            sandbox: None,
            allowed_paths: allowed.map(|v| v.into_iter().map(PathBuf::from).collect()),
            cancel: None,
            session_id: None,
            extra_env: HashMap::new(),
            agent_label: None,
            tab_id: None,
        }
    }

    #[test]
    fn no_restrictions_allows_everything() {
        let ctx = make_ctx(None);
        assert!(ctx.check_path_allowed("/etc/passwd".as_ref()).is_ok());
        assert!(
            ctx.check_path_allowed("/home/user/file.txt".as_ref())
                .is_ok()
        );
    }

    #[test]
    fn empty_vec_allows_everything() {
        // None and Some([]) both mean "no restrictions"
        let ctx = make_ctx(Some(vec![]));
        assert!(ctx.check_path_allowed("/etc/passwd".as_ref()).is_ok());
    }

    #[test]
    fn allowed_dir_permits_children() {
        // Use temp_dir() so macOS resolves /tmp → /private/tmp correctly
        let tmp = std::env::temp_dir();
        let tmp_str = tmp.to_str().unwrap();
        let ctx = make_ctx(Some(vec![tmp_str]));
        assert!(ctx.check_path_allowed(&tmp.join("foo.txt")).is_ok());
        assert!(ctx.check_path_allowed(&tmp.join("sub/dir/file.rs")).is_ok());
    }

    #[test]
    fn blocks_outside_allowed_dirs() {
        let tmp = std::env::temp_dir();
        let tmp_str = tmp.to_str().unwrap();
        let ctx = make_ctx(Some(vec![tmp_str]));
        let result = ctx.check_path_allowed("/etc/passwd".as_ref());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("outside allowed directories"));
    }

    #[test]
    fn multiple_allowed_dirs() {
        let tmp = std::env::temp_dir();
        let tmp_str = tmp.to_str().unwrap();
        let ctx = make_ctx(Some(vec![tmp_str, "/home/enzo/projects"]));
        assert!(ctx.check_path_allowed(&tmp.join("file")).is_ok());
        assert!(
            ctx.check_path_allowed("/home/enzo/projects/foo.rs".as_ref())
                .is_ok()
        );
        assert!(ctx.check_path_allowed("/etc/secret".as_ref()).is_err());
    }

    // ── check_sensitive tests ───────────────────────────────────────

    #[test]
    fn sensitive_blocks_env_file() {
        let ctx = make_ctx(None);
        assert!(ctx.check_sensitive("/home/user/.env".as_ref()).is_err());
        assert!(
            ctx.check_sensitive("/home/user/.env.local".as_ref())
                .is_err()
        );
        assert!(
            ctx.check_sensitive("/home/user/.env.production".as_ref())
                .is_err()
        );
    }

    #[test]
    fn sensitive_blocks_key_files() {
        let ctx = make_ctx(None);
        assert!(
            ctx.check_sensitive("/home/user/server.pem".as_ref())
                .is_err()
        );
        assert!(
            ctx.check_sensitive("/home/user/private.key".as_ref())
                .is_err()
        );
        assert!(ctx.check_sensitive("/home/user/cert.p12".as_ref()).is_err());
        assert!(
            ctx.check_sensitive("/home/user/credentials.json".as_ref())
                .is_err()
        );
        assert!(ctx.check_sensitive("/home/user/.npmrc".as_ref()).is_err());
    }

    #[test]
    fn sensitive_allows_normal_files() {
        let ctx = make_ctx(None);
        assert!(
            ctx.check_sensitive("/home/user/src/main.rs".as_ref())
                .is_ok()
        );
        assert!(ctx.check_sensitive("/home/user/README.md".as_ref()).is_ok());
        assert!(
            ctx.check_sensitive("/home/user/Cargo.toml".as_ref())
                .is_ok()
        );
    }

    #[test]
    fn sensitive_uses_custom_patterns_from_sandbox() {
        let mut ctx = make_ctx(None);
        ctx.sandbox = Some(sandbox::SandboxConfig {
            enabled: true,
            allowed_dirs: vec![],
            sensitive_patterns: vec!["*.secret".to_string(), "passwords*".to_string()],
        });

        // Custom patterns should block
        assert!(
            ctx.check_sensitive("/home/user/db.secret".as_ref())
                .is_err()
        );
        assert!(
            ctx.check_sensitive("/home/user/passwords.txt".as_ref())
                .is_err()
        );

        // Default patterns should NOT block (custom overrides defaults)
        assert!(ctx.check_sensitive("/home/user/.env".as_ref()).is_ok());
    }

    #[test]
    fn sensitive_blocks_ssh_directory() {
        let ctx = make_ctx(None);
        assert!(
            ctx.check_sensitive("/home/user/.ssh/id_rsa".as_ref())
                .is_err()
        );
        assert!(
            ctx.check_sensitive("/home/user/.ssh/known_hosts".as_ref())
                .is_err()
        );
        assert!(
            ctx.check_sensitive("/home/user/.ssh/config".as_ref())
                .is_err()
        );
    }

    #[test]
    fn sensitive_blocks_other_sensitive_dirs() {
        let ctx = make_ctx(None);
        assert!(
            ctx.check_sensitive("/home/user/.gnupg/pubring.kbx".as_ref())
                .is_err()
        );
        assert!(
            ctx.check_sensitive("/home/user/.aws/credentials".as_ref())
                .is_err()
        );
        assert!(
            ctx.check_sensitive("/home/user/.docker/config.json".as_ref())
                .is_err()
        );
    }

    #[test]
    fn sensitive_custom_dir_patterns() {
        let mut ctx = make_ctx(None);
        ctx.sandbox = Some(sandbox::SandboxConfig {
            enabled: true,
            allowed_dirs: vec![],
            sensitive_patterns: vec!["secrets/".to_string(), "*.key".to_string()],
        });

        // Custom dir pattern blocks subtree
        assert!(
            ctx.check_sensitive("/home/user/secrets/api_token".as_ref())
                .is_err()
        );
        // Custom file pattern
        assert!(ctx.check_sensitive("/home/user/tls.key".as_ref()).is_err());
        // Default dir patterns should NOT block (custom overrides)
        assert!(
            ctx.check_sensitive("/home/user/.ssh/id_rsa".as_ref())
                .is_ok()
        );
    }

    #[test]
    fn sensitive_error_message_includes_pattern() {
        let ctx = make_ctx(None);
        let err = ctx.check_sensitive("/home/user/.env".as_ref()).unwrap_err();
        assert!(err.contains("sensitive pattern"));
        assert!(err.contains(".env*"));

        let err = ctx
            .check_sensitive("/home/user/.ssh/id_rsa".as_ref())
            .unwrap_err();
        assert!(err.contains(".ssh/"));
    }
}

// ── Additional unit tests ────────────────────────────────────────────────

#[cfg(test)]
mod format_and_registry_tests {
    use super::*;

    // ── format_stats tests ───────────────────────────────────────────

    #[test]
    fn format_stats_no_changes() {
        assert_eq!(format_stats(0, 0), "No changes");
    }

    #[test]
    fn format_stats_only_added() {
        assert_eq!(format_stats(5, 0), "Added 5 lines");
        assert_eq!(format_stats(1, 0), "Added 1 lines");
    }

    #[test]
    fn format_stats_only_removed() {
        assert_eq!(format_stats(0, 3), "Removed 3 lines");
    }

    #[test]
    fn format_stats_both() {
        assert_eq!(format_stats(5, 2), "Added 5 lines, removed 2 lines");
    }

    // ── ToolRegistry tests ───────────────────────────────────────────

    #[test]
    fn registry_new_is_empty() {
        let registry = ToolRegistry::new();
        assert!(registry.definitions().is_empty());
    }

    #[test]
    fn registry_default_is_empty() {
        let registry = ToolRegistry::default();
        assert!(registry.definitions().is_empty());
    }

    #[test]
    fn registry_get_nonexistent_returns_none() {
        let registry = ToolRegistry::new();
        assert!(registry.get("NonExistent").is_none());
    }

    #[test]
    fn registry_definitions_are_sorted() {
        let registry = create_default_registry();
        let defs = registry.definitions();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "Tool definitions should be sorted by name");
    }

    #[test]
    fn registry_contains_core_tools() {
        let registry = create_default_registry();
        let expected = [
            "Bash",
            "ReadFiles",
            "WriteFile",
            "EditFile",
            "Grep",
            "Glob",
            "WebFetch",
            "TaskAdd",
            "TaskList",
            "TaskUpdate",
            "SubmitPlan",
            "PlannerQuestion",
            "LoadSkill",
        ];
        for name in &expected {
            assert!(
                registry.get(name).is_some(),
                "Registry should contain tool '{name}'"
            );
        }
    }

    #[test]
    fn registry_retain_filters_tools() {
        let mut registry = create_default_registry();
        registry.retain(&["Bash", "Grep"]);
        assert!(registry.get("Bash").is_some());
        assert!(registry.get("Grep").is_some());
        assert!(registry.get("ReadFiles").is_none());
        assert!(registry.get("EditFile").is_none());
        assert_eq!(registry.definitions().len(), 2);
    }

    #[test]
    fn registry_retain_empty_removes_all() {
        let mut registry = create_default_registry();
        registry.retain(&[]);
        assert!(registry.definitions().is_empty());
    }

    #[test]
    fn registry_sandbox_settings() {
        let mut registry = ToolRegistry::new();
        assert!(!registry.is_sandbox_enabled());
        assert!(registry.allowed_dirs().is_empty());
        assert!(registry.sensitive_patterns().is_empty());

        registry.set_sandbox(
            true,
            vec!["/home/user/project".to_string()],
            vec!["*.pem".to_string(), ".env*".to_string()],
        );
        assert!(registry.is_sandbox_enabled());
        assert_eq!(registry.allowed_dirs(), &["/home/user/project"]);
        assert_eq!(
            registry.sensitive_patterns(),
            &["*.pem".to_string(), ".env*".to_string()]
        );
    }

    #[test]
    fn tool_definitions_have_valid_schemas() {
        let registry = create_default_registry();
        for def in registry.definitions() {
            assert!(!def.name.is_empty(), "Tool name should not be empty");
            assert!(
                !def.description.is_empty(),
                "Tool '{}' description should not be empty",
                def.name
            );
            // Input schema should be a JSON object with "type": "object"
            assert_eq!(
                def.input_schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "Tool '{}' input_schema should have type: object",
                def.name
            );
        }
    }

    #[test]
    fn all_tool_names_returns_sorted() {
        let names = all_tool_names();
        assert!(!names.is_empty());
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "all_tool_names should return sorted names");
    }

    #[test]
    fn all_tool_info_built_in_have_no_source() {
        let info = all_tool_info();
        // Built-in tools should have source = None
        let bash = info.iter().find(|t| t.name == "Bash");
        assert!(bash.is_some());
        assert!(
            bash.unwrap().source.is_none(),
            "Built-in Bash tool should have no source"
        );
    }
}

/// Format diff stats like "Added 5 lines, removed 2 lines"
pub(crate) fn format_stats(added: usize, removed: usize) -> String {
    match (added, removed) {
        (0, 0) => "No changes".to_string(),
        (a, 0) => format!("Added {a} lines"),
        (0, r) => format!("Removed {r} lines"),
        (a, r) => format!("Added {a} lines, removed {r} lines"),
    }
}

/// List all available tool names (core + plugin-provided).
/// Lightweight: only reads tool names without full initialization.
pub fn all_tool_names() -> Vec<String> {
    let registry = create_default_registry();
    let mut names: Vec<String> = registry.tools.keys().cloned().collect();
    names.sort();
    names
}

/// Tool info with optional source plugin name.
#[derive(serde::Serialize)]
pub struct ToolInfo {
    pub name: String,
    /// Plugin name if provided by a plugin, null for built-in tools
    pub source: Option<String>,
}

/// List all available tools with their source (plugin name or null for built-in).
pub fn all_tool_info() -> Vec<ToolInfo> {
    tool_info_from_registry(create_default_registry())
}

/// Like `all_tool_info` but also includes browser tools.
/// Call this when the browser has been launched/activated.
pub fn all_tool_info_with_browser() -> Vec<ToolInfo> {
    tool_info_from_registry(create_browser_registry())
}

fn tool_info_from_registry(registry: ToolRegistry) -> Vec<ToolInfo> {
    let mut tools: Vec<ToolInfo> = registry
        .tools
        .values()
        .map(|t| ToolInfo {
            name: t.name().to_string(),
            source: t.source().map(|s| s.to_string()),
        })
        .collect();
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    tools
}

/// Create a registry with all default tools
pub fn create_default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(bash::BashTool));
    registry.register(Box::new(read_file::ReadFileTool));
    registry.register(Box::new(write_file::WriteFileTool));
    registry.register(Box::new(edit_file::EditFileTool));
    registry.register(Box::new(grep::GrepTool));
    registry.register(Box::new(glob_tool::GlobTool));
    registry.register(Box::new(web_fetch::WebFetchTool));
    // Task tools
    registry.register(Box::new(tasks::TaskAddTool));
    registry.register(Box::new(tasks::TaskListTool));
    registry.register(Box::new(tasks::TaskUpdateTool));
    // Planner tools (intercepted by agent loop)
    registry.register(Box::new(planner_tools::SubmitPlanTool));
    registry.register(Box::new(planner_tools::PlannerQuestionTool));
    // Skill loading tool
    registry.register(Box::new(skills::LoadSkillTool));
    // Plugin-provided tools (scanned from installed plugins)
    plugin_tools::register_plugin_tools(&mut registry);
    registry
}

/// Create a registry with all default tools plus browser tools.
pub fn create_browser_registry() -> ToolRegistry {
    let mut registry = create_default_registry();
    browser::register_browser_tools(&mut registry);
    registry
}

/// Create a browser-enabled registry configured with project permissions.
/// `allowed_dirs` are extra directories that should be writable inside the
/// bwrap sandbox (from `allowedPaths` in `.lukan/config.json`).
pub fn create_configured_browser_registry(
    permissions: &lukan_core::config::types::PermissionsConfig,
    allowed_dirs: &[std::path::PathBuf],
) -> ToolRegistry {
    let mut registry = create_browser_registry();
    registry.set_sandbox(
        permissions.os_sandbox,
        allowed_dirs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        permissions.sensitive_patterns.clone(),
    );
    registry
}

/// Result of MCP initialization.
pub struct McpInitResult {
    /// The manager (must be kept alive for tool proxies to work).
    pub manager: mcp::McpManager,
    /// Number of tools successfully registered.
    pub tool_count: usize,
    /// Per-server errors encountered during init.
    pub errors: Vec<(String, String)>,
}

/// Initialize MCP servers and register their tools into a registry.
///
/// Returns the `McpManager` which must be kept alive for the tool proxies to work,
/// along with diagnostic info about what was loaded.
pub async fn init_mcp_tools(
    registry: &mut ToolRegistry,
    configs: &std::collections::HashMap<String, lukan_core::config::types::McpServerConfig>,
) -> McpInitResult {
    let (manager, errors) = mcp::McpManager::init(configs).await;
    let tool_count = manager.tool_defs.len();
    mcp_tools::register_mcp_tools(registry, &manager);
    McpInitResult {
        manager,
        tool_count,
        errors,
    }
}

/// Create a registry configured with project permissions.
/// `allowed_dirs` are extra directories that should be writable inside the
/// bwrap sandbox (from `allowedPaths` in `.lukan/config.json`).
pub fn create_configured_registry(
    permissions: &lukan_core::config::types::PermissionsConfig,
    allowed_dirs: &[std::path::PathBuf],
) -> ToolRegistry {
    let mut registry = create_default_registry();
    registry.set_sandbox(
        permissions.os_sandbox,
        allowed_dirs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        permissions.sensitive_patterns.clone(),
    );
    registry
}
