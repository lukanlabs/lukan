mod test_helpers;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use lukan_tools::{Tool, ToolRegistry};
use serde_json::json;
use test_helpers::make_tool_context;

struct FakeTool {
    name: &'static str,
    available: bool,
}

#[async_trait]
impl Tool for FakeTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "fake tool"
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    fn is_available(&self) -> bool {
        self.available
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &lukan_tools::ToolContext,
    ) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success(format!("{}:{}", self.name, input)))
    }
}

#[test]
fn register_skips_unavailable_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(FakeTool {
        name: "Unavailable",
        available: false,
    }));

    assert!(registry.get("Unavailable").is_none());
}

#[test]
fn get_returns_registered_tool_by_name() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(FakeTool {
        name: "Available",
        available: true,
    }));

    let tool = registry.get("Available");
    assert!(tool.is_some());
    assert_eq!(tool.unwrap().name(), "Available");
}

#[tokio::test]
async fn execute_calls_the_matching_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(FakeTool {
        name: "ExecTool",
        available: true,
    }));

    let cwd = std::env::temp_dir();
    let ctx = make_tool_context(&cwd);
    let result = registry
        .execute("ExecTool", json!({"value": 42}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert!(result.content.contains("ExecTool"));
    assert!(result.content.contains("42"));
}

#[tokio::test]
async fn execute_unknown_tool_returns_error_result() {
    let registry = ToolRegistry::new();
    let cwd = std::env::temp_dir();
    let ctx = make_tool_context(&cwd);

    let result = registry.execute("MissingTool", json!({}), &ctx).await.unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("Unknown tool: MissingTool"));
}
