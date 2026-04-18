use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use lukan_tools::tool_search::search_deferred_tools;
use lukan_tools::{Tool, ToolRegistry};
use serde_json::json;

struct DeferredPluginTool;
struct DeferredMcpTool;

#[async_trait]
impl Tool for DeferredPluginTool {
    fn name(&self) -> &str {
        "PluginSpecialTool"
    }

    fn description(&self) -> &str {
        "plugin-only specialized capability"
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    fn source(&self) -> Option<&str> {
        Some("demo-plugin")
    }

    fn is_deferred(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &lukan_tools::ToolContext,
    ) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success("plugin"))
    }
}

#[async_trait]
impl Tool for DeferredMcpTool {
    fn name(&self) -> &str {
        "mcp__github__open_issue"
    }

    fn description(&self) -> &str {
        "open an issue in github"
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    fn source(&self) -> Option<&str> {
        Some("mcp:github")
    }

    fn is_deferred(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &lukan_tools::ToolContext,
    ) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success("mcp"))
    }
}


#[test]
fn search_deferred_tools_prioritizes_exact_name_match() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(DeferredPluginTool));

    let results = search_deferred_tools(&registry, "PluginSpecialTool", 5);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "PluginSpecialTool");
}

#[test]
fn search_deferred_tools_understands_mcp_name_parts() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(DeferredMcpTool));

    let results = search_deferred_tools(&registry, "github open issue", 5);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "mcp__github__open_issue");
}
