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
fn metadata_defaults_for_fake_tool_are_conservative() {
    let tool = FakeTool {
        name: "DefaultMeta",
        available: true,
    };

    assert!(!tool.is_read_only());
    assert!(!tool.is_concurrency_safe());
    assert_eq!(tool.search_hint(), None);
    assert_eq!(tool.activity_label(&json!({})), None);
    assert!(!tool.is_deferred());
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

#[test]
fn built_in_tool_metadata_matches_stage_one_expectations() {
    let registry = lukan_tools::create_default_registry();

    let read = registry.get("ReadFiles").unwrap();
    assert!(read.is_read_only());
    assert!(read.is_concurrency_safe());
    assert_eq!(read.search_hint(), Some("read file contents with numbered lines"));
    assert_eq!(read.activity_label(&json!({})), Some("Reading file".to_string()));

    let glob = registry.get("Glob").unwrap();
    assert!(glob.is_read_only());
    assert!(glob.is_concurrency_safe());
    assert_eq!(glob.search_hint(), Some("find files by glob pattern"));
    assert_eq!(glob.activity_label(&json!({})), Some("Finding files".to_string()));

    let edit = registry.get("EditFile").unwrap();
    assert!(!edit.is_read_only());
    assert!(!edit.is_concurrency_safe());
    assert_eq!(
        edit.search_hint(),
        Some("edit existing files by exact string replacement")
    );
    assert_eq!(edit.activity_label(&json!({})), Some("Editing file".to_string()));

    let bash = registry.get("Bash").unwrap();
    assert!(!bash.is_read_only());
    assert!(!bash.is_concurrency_safe());
    assert_eq!(bash.search_hint(), Some("run shell commands and terminal tasks"));
    assert_eq!(bash.activity_label(&json!({})), Some("Running command".to_string()));

    let web_fetch = registry.get("WebFetch").unwrap();
    assert!(web_fetch.is_read_only());
    assert!(web_fetch.is_concurrency_safe());
    assert_eq!(web_fetch.search_hint(), Some("fetch content from a URL"));
    assert_eq!(
        web_fetch.activity_label(&json!({})),
        Some("Fetching web page".to_string())
    );

    let grep = registry.get("Grep").unwrap();
    assert!(grep.is_read_only());
    assert!(grep.is_concurrency_safe());
    assert_eq!(grep.search_hint(), Some("search file contents by regex"));
    assert_eq!(grep.activity_label(&json!({})), Some("Searching files".to_string()));
    assert!(!grep.is_deferred());

    let write = registry.get("WriteFile").unwrap();
    assert!(!write.is_read_only());
    assert!(!write.is_concurrency_safe());
    assert_eq!(write.search_hint(), Some("write a file to disk"));
    assert_eq!(write.activity_label(&json!({})), Some("Writing file".to_string()));

    let remember = registry.get("Remember").unwrap();
    assert!(remember.is_read_only());
    assert!(remember.is_concurrency_safe());
    assert_eq!(
        remember.search_hint(),
        Some("recall project decisions and lessons learned")
    );
    assert_eq!(
        remember.activity_label(&json!({})),
        Some("Recalling memories".to_string())
    );

    let load_skill = registry.get("LoadSkill").unwrap();
    assert!(load_skill.is_read_only());
    assert!(load_skill.is_concurrency_safe());
    assert_eq!(
        load_skill.search_hint(),
        Some("load project-specific skill instructions")
    );
    assert_eq!(
        load_skill.activity_label(&json!({})),
        Some("Loading skill".to_string())
    );
}
