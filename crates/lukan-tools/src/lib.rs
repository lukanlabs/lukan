pub mod bg_processes;
mod bash;
mod edit_file;
mod glob_tool;
mod grep;
mod read_file;
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
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
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

/// Format diff stats like "Added 5 lines, removed 2 lines"
pub(crate) fn format_stats(added: usize, removed: usize) -> String {
    match (added, removed) {
        (0, 0) => "No changes".to_string(),
        (a, 0) => format!("Added {a} lines"),
        (0, r) => format!("Removed {r} lines"),
        (a, r) => format!("Added {a} lines, removed {r} lines"),
    }
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
    registry
}
