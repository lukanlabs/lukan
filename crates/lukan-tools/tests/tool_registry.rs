mod test_helpers;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use lukan_tools::{BashCommandClass, Tool, ToolRegistry, classify_bash_command};
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

struct ValidatingTool;

#[async_trait]
impl Tool for ValidatingTool {
    fn name(&self) -> &str {
        "ValidatingTool"
    }

    fn description(&self) -> &str {
        "tool with validation"
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({"type": "object"})
    }

    fn validate_input(&self, input: &serde_json::Value, _ctx: &lukan_tools::ToolContext) -> Result<(), String> {
        match input.get("allowed").and_then(|v| v.as_bool()) {
            Some(true) => Ok(()),
            _ => Err("validation failed".to_string()),
        }
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &lukan_tools::ToolContext,
    ) -> anyhow::Result<ToolResult> {
        Ok(ToolResult::success("executed"))
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

#[tokio::test]
async fn execute_returns_validation_error_before_running_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ValidatingTool));
    let cwd = std::env::temp_dir();
    let ctx = make_tool_context(&cwd);

    let result = registry
        .execute("ValidatingTool", json!({"allowed": false}), &ctx)
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("validation failed"));
}

#[tokio::test]
async fn execute_runs_validating_tool_when_input_is_valid() {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(ValidatingTool));
    let cwd = std::env::temp_dir();
    let ctx = make_tool_context(&cwd);

    let result = registry
        .execute("ValidatingTool", json!({"allowed": true}), &ctx)
        .await
        .unwrap();

    assert!(!result.is_error);
    assert_eq!(result.content, "executed");
}

#[test]
fn classify_bash_command_covers_common_risk_levels() {
    assert_eq!(classify_bash_command("git status"), BashCommandClass::Read);
    assert_eq!(classify_bash_command("find src -name '*.rs'"), BashCommandClass::Search);
    assert_eq!(classify_bash_command("ls -la"), BashCommandClass::List);
    assert_eq!(classify_bash_command("curl https://lukan.ai"), BashCommandClass::Network);
    assert_eq!(classify_bash_command("mkdir tmp"), BashCommandClass::Mutating);
    assert_eq!(
        classify_bash_command("git reset --hard HEAD~1"),
        BashCommandClass::Destructive
    );
    assert_eq!(classify_bash_command("echo hello"), BashCommandClass::Unknown);
}

#[test]
fn classify_bash_command_detects_mutating_redirections_and_builds() {
    assert_eq!(classify_bash_command("echo hi > out.txt"), BashCommandClass::Mutating);
    assert_eq!(
        classify_bash_command("cargo test -p lukan-agent"),
        BashCommandClass::Mutating
    );
    assert_eq!(classify_bash_command("python -c 'print(1)'"), BashCommandClass::Mutating);
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
    assert_eq!(bash.search_hint(), Some("run shell commands and terminal tasks; read/search/list commands are lower risk than mutating or destructive ones"));
    assert_eq!(
        bash.activity_label(&json!({"command": "git status"})),
        Some("[read] git status".to_string())
    );
    assert_eq!(bash.activity_label(&json!({})), Some("Running command".to_string()));

    let web_fetch = registry.get("WebFetch").unwrap();
    assert!(web_fetch.is_read_only());
    assert!(web_fetch.is_concurrency_safe());
    assert!(web_fetch.is_deferred());
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

    let web_search = registry.get("WebSearch");
    if let Some(web_search) = web_search {
        assert!(web_search.is_read_only());
        assert!(web_search.is_concurrency_safe());
        assert!(web_search.is_deferred());
        assert_eq!(web_search.search_hint(), Some("search the web for information"));
        assert_eq!(
            web_search.activity_label(&json!({})),
            Some("Searching web".to_string())
        );
    }

    let task_add = registry.get("TaskAdd").unwrap();
    assert!(!task_add.is_read_only());
    assert!(!task_add.is_concurrency_safe());
    assert_eq!(task_add.search_hint(), Some("add tasks to the task list"));
    assert_eq!(task_add.activity_label(&json!({})), Some("Adding tasks".to_string()));

    let task_list = registry.get("TaskList").unwrap();
    assert!(task_list.is_read_only());
    assert!(task_list.is_concurrency_safe());
    assert_eq!(task_list.search_hint(), Some("list current tasks and statuses"));
    assert_eq!(task_list.activity_label(&json!({})), Some("Listing tasks".to_string()));

    let task_update = registry.get("TaskUpdate").unwrap();
    assert!(!task_update.is_read_only());
    assert!(!task_update.is_concurrency_safe());
    assert_eq!(task_update.search_hint(), Some("update task status or title"));
    assert_eq!(
        task_update.activity_label(&json!({})),
        Some("Updating tasks".to_string())
    );

    let submit_plan = registry.get("SubmitPlan").unwrap();
    assert!(!submit_plan.is_read_only());
    assert!(!submit_plan.is_concurrency_safe());
    assert_eq!(
        submit_plan.search_hint(),
        Some("submit a structured implementation plan")
    );
    assert_eq!(
        submit_plan.activity_label(&json!({})),
        Some("Submitting plan".to_string())
    );

    let planner_question = registry.get("PlannerQuestion").unwrap();
    assert!(planner_question.is_read_only());
    assert!(!planner_question.is_concurrency_safe());
    assert_eq!(
        planner_question.search_hint(),
        Some("ask the user clarifying planner questions")
    );
    let browser_snapshot = registry.get("BrowserSnapshot");
    if let Some(browser_snapshot) = browser_snapshot {
        assert!(browser_snapshot.is_read_only());
        assert!(!browser_snapshot.is_concurrency_safe());
        assert!(browser_snapshot.is_deferred());
        assert_eq!(
            browser_snapshot.search_hint(),
            Some("capture an accessibility snapshot of the current page")
        );
        assert_eq!(
            browser_snapshot.activity_label(&json!({})),
            Some("Snapshotting page".to_string())
        );
    }

    let browser_click = registry.get("BrowserClick");
    if let Some(browser_click) = browser_click {
        assert!(!browser_click.is_read_only());
        assert!(!browser_click.is_concurrency_safe());
        assert!(browser_click.is_deferred());
        assert_eq!(
            browser_click.search_hint(),
            Some("click an element in the browser")
        );
        assert_eq!(
            browser_click.activity_label(&json!({})),
            Some("Clicking element".to_string())
        );
    }

    let browser_evaluate = registry.get("BrowserEvaluate");
    if let Some(browser_evaluate) = browser_evaluate {
        assert!(browser_evaluate.is_read_only());
        assert!(!browser_evaluate.is_concurrency_safe());
        assert!(browser_evaluate.is_deferred());
        assert_eq!(
            browser_evaluate.search_hint(),
            Some("evaluate JavaScript in the browser page")
        );
        assert_eq!(
            browser_evaluate.activity_label(&json!({})),
            Some("Evaluating browser script".to_string())
        );
    }
}
