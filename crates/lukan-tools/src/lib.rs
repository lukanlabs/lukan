mod bash;
pub mod bg_processes;
pub mod browser;
mod edit_file;
mod glob_tool;
mod grep;
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
        let ctx = make_ctx(Some(vec!["/tmp"]));
        assert!(ctx.check_path_allowed("/tmp/foo.txt".as_ref()).is_ok());
        assert!(
            ctx.check_path_allowed("/tmp/sub/dir/file.rs".as_ref())
                .is_ok()
        );
    }

    #[test]
    fn blocks_outside_allowed_dirs() {
        let ctx = make_ctx(Some(vec!["/tmp"]));
        let result = ctx.check_path_allowed("/etc/passwd".as_ref());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("outside allowed directories"));
    }

    #[test]
    fn multiple_allowed_dirs() {
        let ctx = make_ctx(Some(vec!["/tmp", "/home/enzo/projects"]));
        assert!(ctx.check_path_allowed("/tmp/file".as_ref()).is_ok());
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
