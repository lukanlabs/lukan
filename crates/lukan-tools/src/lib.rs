mod bash;
pub mod bg_processes;
mod edit_file;
mod glob_tool;
mod grep;
pub mod plugin_tools;
mod read_file;
pub mod sandbox;
mod web_fetch;
mod write_file;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use lukan_core::models::tools::{ToolDefinition, ToolResult};
use tokio::sync::{Mutex, mpsc, watch};

/// Context passed to every tool execution
pub struct ToolContext {
    /// Optional channel for progress updates
    pub progress_tx: Option<mpsc::Sender<String>>,
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
}

/// Trait that all tools must implement
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique tool name (e.g. "Bash", "ReadFile")
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// JSON Schema for the tool's input
    fn input_schema(&self) -> serde_json::Value;

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
            read_files: Arc::new(Mutex::new(HashSet::new())),
            cwd: PathBuf::from("/tmp"),
            bg_signal: None,
            sandbox: None,
            allowed_paths: allowed.map(|v| v.into_iter().map(PathBuf::from).collect()),
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
    // Plugin-provided tools (scanned from installed plugins)
    plugin_tools::register_plugin_tools(&mut registry);
    registry
}
