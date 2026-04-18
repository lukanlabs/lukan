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
fn search_deferred_tools_can_find_plugin_and_mcp_entries() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(DeferredPluginTool));
    registry.register(Box::new(DeferredMcpTool));

    let plugin_results = search_deferred_tools(&registry, "plugin specialized", 5);
    assert!(plugin_results.iter().any(|r| r.name == "PluginSpecialTool"));

    let mcp_results = search_deferred_tools(&registry, "github issue", 5);
    assert!(mcp_results.iter().any(|r| r.name == "mcp__github__open_issue"));
}
