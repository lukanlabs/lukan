//! MCP tool proxies — wraps MCP server tools as native `Tool` trait impls.

use std::sync::Arc;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use tokio::sync::Mutex;
use tracing::debug;

use crate::mcp::{McpClient, McpManager, McpToolDef};
use crate::{Tool, ToolContext, ToolRegistry};

/// A tool provided by an MCP server, implementing the `Tool` trait.
pub struct McpProvidedTool {
    server_name: String,
    tool_def: McpToolDef,
    client: Arc<Mutex<McpClient>>,
}

impl McpProvidedTool {
    pub fn new(server_name: String, tool_def: McpToolDef, client: Arc<Mutex<McpClient>>) -> Self {
        Self {
            server_name,
            tool_def,
            client,
        }
    }
}

#[async_trait]
impl Tool for McpProvidedTool {
    fn name(&self) -> &str {
        &self.tool_def.name
    }

    fn description(&self) -> &str {
        &self.tool_def.description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.tool_def.input_schema.clone()
    }

    fn source(&self) -> Option<&str> {
        // Leak a string to return a stable reference. This is acceptable
        // because tools are long-lived and created once at startup.
        let s: &str = Box::leak(format!("mcp:{}", self.server_name).into_boxed_str());
        Some(s)
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn is_deferred(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        debug!(
            server = %self.server_name,
            tool = %self.tool_def.name,
            "Calling MCP tool"
        );

        let mut client = self.client.lock().await;
        match client.call_tool(&self.tool_def.name, input).await {
            Ok(result) => {
                if result.is_error {
                    Ok(ToolResult::error(result.content))
                } else {
                    Ok(ToolResult::success(result.content))
                }
            }
            Err(e) => Ok(ToolResult::error(format!(
                "MCP tool '{}' failed: {e}",
                self.tool_def.name
            ))),
        }
    }
}

/// Register all MCP tools from a manager into a tool registry.
pub fn register_mcp_tools(registry: &mut ToolRegistry, manager: &McpManager) {
    for (server_name, tool_def) in &manager.tool_defs {
        if let Some(client) = manager.get_client(server_name) {
            let tool = McpProvidedTool::new(server_name.clone(), tool_def.clone(), client);
            registry.register(Box::new(tool));
        }
    }
}
