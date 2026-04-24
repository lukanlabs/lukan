//! Task tracking tools: TaskAdd, TaskList, TaskUpdate.
//!
//! Tasks are stored in `.lukan/tasks.md` (relative to cwd) in markdown format:
//! ```text
//! # Tasks
//!
//! - [ ] **1** — Pending task
//! - [~] **2** — In progress task
//! - [x] **3** — Done task
//! ```

use std::path::Path;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{Tool, ToolContext};

// ── Types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Done,
}

impl TaskStatus {
    fn marker(&self) -> &str {
        match self {
            Self::Pending => " ",
            Self::InProgress => "~",
            Self::Done => "x",
        }
    }

    fn icon(&self) -> &str {
        match self {
            Self::Pending => "⏳",
            Self::InProgress => "🔄",
            Self::Done => "✅",
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Done => "done",
        }
    }

    fn from_marker(c: &str) -> Option<Self> {
        match c {
            " " => Some(Self::Pending),
            "~" => Some(Self::InProgress),
            "x" => Some(Self::Done),
            _ => None,
        }
    }

    fn from_label(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "in_progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub id: u32,
    pub title: String,
    pub status: TaskStatus,
}

// ── Mutex for serialized access ──────────────────────────────────────────

static TASK_LOCK: Mutex<()> = Mutex::const_new(());

// ── File helpers ─────────────────────────────────────────────────────────

fn tasks_path(cwd: &Path) -> std::path::PathBuf {
    cwd.join(".lukan").join("tasks.md")
}

pub fn parse_tasks(content: &str) -> Vec<TaskEntry> {
    let re = regex::Regex::new(r"^- \[([ ~x])\] \*\*(\d+)\*\*\s*[—–\-]\s*(.+)$").unwrap();
    let mut entries = Vec::new();
    for line in content.lines() {
        if let Some(caps) = re.captures(line) {
            let marker = caps.get(1).unwrap().as_str();
            let id: u32 = caps.get(2).unwrap().as_str().parse().unwrap_or(0);
            let title = caps.get(3).unwrap().as_str().trim().to_string();
            if let Some(status) = TaskStatus::from_marker(marker) {
                entries.push(TaskEntry { id, title, status });
            }
        }
    }
    entries
}

fn format_tasks(entries: &[TaskEntry]) -> String {
    if entries.is_empty() {
        return "# Tasks\n\n_No tasks._\n".to_string();
    }
    let lines: Vec<String> = entries
        .iter()
        .map(|e| format!("- [{}] **{}** — {}", e.status.marker(), e.id, e.title))
        .collect();
    format!("# Tasks\n\n{}\n", lines.join("\n"))
}

async fn read_tasks_file(cwd: &Path) -> String {
    let path = tasks_path(cwd);
    tokio::fs::read_to_string(&path).await.unwrap_or_default()
}

async fn write_tasks_file(cwd: &Path, content: &str) {
    let path = tasks_path(cwd);
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let _ = tokio::fs::write(&path, content).await;
}

// ── Public helpers (called from agent loop / TUI) ────────────────────────

/// Read and parse all task entries from `.lukan/tasks.md`.
pub async fn read_all_tasks(cwd: &Path) -> Vec<TaskEntry> {
    let content = read_tasks_file(cwd).await;
    parse_tasks(&content)
}

/// Replace all tasks with ones from an accepted plan.
pub async fn create_tasks_from_plan(cwd: &Path, titles: &[String]) {
    let _lock = TASK_LOCK.lock().await;
    let entries: Vec<TaskEntry> = titles
        .iter()
        .enumerate()
        .map(|(i, title)| TaskEntry {
            id: (i + 1) as u32,
            title: title.clone(),
            status: TaskStatus::Pending,
        })
        .collect();
    write_tasks_file(cwd, &format_tasks(&entries)).await;
}

/// Get tasks formatted for inclusion in the system prompt dynamic section.
/// Returns `None` if there are no active tasks.
pub async fn get_tasks_for_prompt(cwd: &Path) -> Option<String> {
    let content = read_tasks_file(cwd).await;
    let entries = parse_tasks(&content);
    let active: Vec<&TaskEntry> = entries
        .iter()
        .filter(|e| e.status != TaskStatus::Done)
        .collect();
    if active.is_empty() {
        return None;
    }
    let lines: Vec<String> = active
        .iter()
        .map(|e| {
            format!(
                "- {} #{} [{}]: {}",
                e.status.icon(),
                e.id,
                e.status.label(),
                e.title
            )
        })
        .collect();
    Some(lines.join("\n"))
}

// ── TaskAddTool ──────────────────────────────────────────────────────────

pub struct TaskAddTool;

#[async_trait]
impl Tool for TaskAddTool {
    fn name(&self) -> &str {
        "TaskAdd"
    }

    fn description(&self) -> &str {
        "Add new tasks to the task list. Tasks are appended with auto-incrementing IDs."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tasks": {
                    "type": "array",
                    "items": { "type": "string", "minLength": 1 },
                    "minItems": 1,
                    "description": "List of task titles to create"
                }
            },
            "required": ["tasks"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn search_hint(&self) -> Option<&str> {
        Some("add tasks to the task list")
    }

    fn activity_label(&self, _input: &Value) -> Option<String> {
        Some("Adding tasks".to_string())
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let _lock = TASK_LOCK.lock().await;

        let tasks = match input.get("tasks").and_then(|v| v.as_array()) {
            Some(arr) => arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>(),
            None => return Ok(ToolResult::error("Missing or invalid 'tasks' array")),
        };

        if tasks.is_empty() {
            return Ok(ToolResult::error("Tasks array is empty"));
        }

        let content = read_tasks_file(&ctx.cwd).await;
        let mut entries = parse_tasks(&content);
        let next_id = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;

        let mut added = Vec::new();
        for (i, title) in tasks.iter().enumerate() {
            let id = next_id + i as u32;
            entries.push(TaskEntry {
                id,
                title: title.clone(),
                status: TaskStatus::Pending,
            });
            added.push(format!("#{id}: {title}"));
        }

        write_tasks_file(&ctx.cwd, &format_tasks(&entries)).await;
        Ok(ToolResult::success(format!(
            "{} task(s) added:\n{}",
            added.len(),
            added.join("\n")
        )))
    }
}

// ── TaskListTool ─────────────────────────────────────────────────────────

pub struct TaskListTool;

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        "TaskList"
    }

    fn description(&self) -> &str {
        "List tasks with their current status. By default shows only active tasks (pending + in_progress)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "all": {
                    "type": "boolean",
                    "description": "Include completed tasks (default: false)"
                }
            }
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn search_hint(&self) -> Option<&str> {
        Some("list current tasks and statuses")
    }

    fn activity_label(&self, _input: &Value) -> Option<String> {
        Some("Listing tasks".to_string())
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let _lock = TASK_LOCK.lock().await;

        let show_all = input.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
        let content = read_tasks_file(&ctx.cwd).await;
        let entries = parse_tasks(&content);

        let filtered: Vec<&TaskEntry> = if show_all {
            entries.iter().collect()
        } else {
            entries
                .iter()
                .filter(|e| e.status != TaskStatus::Done)
                .collect()
        };

        if filtered.is_empty() {
            return Ok(ToolResult::success("No active tasks."));
        }

        let lines: Vec<String> = filtered
            .iter()
            .map(|e| {
                format!(
                    "{} #{} [{}] — {}",
                    e.status.icon(),
                    e.id,
                    e.status.label(),
                    e.title
                )
            })
            .collect();

        Ok(ToolResult::success(format!(
            "{} task(s):\n\n{}",
            filtered.len(),
            lines.join("\n")
        )))
    }
}

// ── TaskUpdateTool ───────────────────────────────────────────────────────

pub struct TaskUpdateTool;

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        "TaskUpdate"
    }

    fn description(&self) -> &str {
        "Batch update task status and/or title. Use to mark tasks in_progress or done."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "updates": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "integer", "description": "Task ID to update" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "done"],
                                "description": "New status"
                            },
                            "title": { "type": "string", "description": "New title" }
                        },
                        "required": ["id"]
                    },
                    "minItems": 1,
                    "description": "List of task updates"
                }
            },
            "required": ["updates"]
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    fn search_hint(&self) -> Option<&str> {
        Some("update task status or title")
    }

    fn activity_label(&self, _input: &Value) -> Option<String> {
        Some("Updating tasks".to_string())
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let _lock = TASK_LOCK.lock().await;

        let updates = match input.get("updates").and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return Ok(ToolResult::error("Missing or invalid 'updates' array")),
        };

        let content = read_tasks_file(&ctx.cwd).await;
        let mut entries = parse_tasks(&content);
        let mut results = Vec::new();

        for upd in updates {
            let id = match upd.get("id").and_then(|v| v.as_u64()) {
                Some(id) => id as u32,
                None => {
                    results.push("Missing task id".to_string());
                    continue;
                }
            };

            let entry = entries.iter_mut().find(|e| e.id == id);
            match entry {
                Some(entry) => {
                    if let Some(status_str) = upd.get("status").and_then(|v| v.as_str())
                        && let Some(status) = TaskStatus::from_label(status_str)
                    {
                        entry.status = status;
                    }
                    if let Some(title) = upd.get("title").and_then(|v| v.as_str())
                        && !title.is_empty()
                    {
                        entry.title = title.to_string();
                    }
                    results.push(format!(
                        "#{}: {} {}",
                        entry.id,
                        entry.status.icon(),
                        entry.title
                    ));
                }
                None => {
                    results.push(format!("#{id}: not found"));
                }
            }
        }

        write_tasks_file(&ctx.cwd, &format_tasks(&entries)).await;
        Ok(ToolResult::success(format!(
            "{} task(s) updated:\n{}",
            results.len(),
            results.join("\n")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── TaskStatus tests ─────────────────────────────────────────────

    #[test]
    fn task_status_marker_roundtrip() {
        assert_eq!(TaskStatus::from_marker(" "), Some(TaskStatus::Pending));
        assert_eq!(TaskStatus::from_marker("~"), Some(TaskStatus::InProgress));
        assert_eq!(TaskStatus::from_marker("x"), Some(TaskStatus::Done));
        assert_eq!(TaskStatus::from_marker("?"), None);
        assert_eq!(TaskStatus::from_marker("X"), None);
    }

    #[test]
    fn task_status_label_roundtrip() {
        assert_eq!(TaskStatus::from_label("pending"), Some(TaskStatus::Pending));
        assert_eq!(
            TaskStatus::from_label("in_progress"),
            Some(TaskStatus::InProgress)
        );
        assert_eq!(TaskStatus::from_label("done"), Some(TaskStatus::Done));
        assert_eq!(TaskStatus::from_label("unknown"), None);
        assert_eq!(TaskStatus::from_label(""), None);
    }

    #[test]
    fn task_status_marker_values() {
        assert_eq!(TaskStatus::Pending.marker(), " ");
        assert_eq!(TaskStatus::InProgress.marker(), "~");
        assert_eq!(TaskStatus::Done.marker(), "x");
    }

    #[test]
    fn task_status_label_values() {
        assert_eq!(TaskStatus::Pending.label(), "pending");
        assert_eq!(TaskStatus::InProgress.label(), "in_progress");
        assert_eq!(TaskStatus::Done.label(), "done");
    }

    #[test]
    fn task_status_icon_values() {
        // Just verify they return non-empty strings
        assert!(!TaskStatus::Pending.icon().is_empty());
        assert!(!TaskStatus::InProgress.icon().is_empty());
        assert!(!TaskStatus::Done.icon().is_empty());
    }

    // ── parse_tasks tests ────────────────────────────────────────────

    #[test]
    fn parse_tasks_empty_content() {
        assert!(parse_tasks("").is_empty());
    }

    #[test]
    fn parse_tasks_no_task_lines() {
        let content = "# Tasks\n\nSome random text\n";
        assert!(parse_tasks(content).is_empty());
    }

    #[test]
    fn parse_tasks_single_pending() {
        let content = "- [ ] **1** — Implement feature X\n";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, 1);
        assert_eq!(tasks[0].title, "Implement feature X");
        assert_eq!(tasks[0].status, TaskStatus::Pending);
    }

    #[test]
    fn parse_tasks_all_statuses() {
        let content = "\
- [ ] **1** — Pending task
- [~] **2** — In progress task
- [x] **3** — Done task
";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].status, TaskStatus::Pending);
        assert_eq!(tasks[1].status, TaskStatus::InProgress);
        assert_eq!(tasks[2].status, TaskStatus::Done);
    }

    #[test]
    fn parse_tasks_with_header_and_extra_lines() {
        let content = "\
# Tasks

- [ ] **1** — First task
- [x] **2** — Second task

_No more tasks._
";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, 1);
        assert_eq!(tasks[1].id, 2);
    }

    #[test]
    fn parse_tasks_supports_dash_separator() {
        // The regex supports em-dash, en-dash, and regular dash
        let content = "- [ ] **1** - Task with dash\n";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Task with dash");
    }

    #[test]
    fn parse_tasks_supports_en_dash() {
        let content = "- [ ] **1** \u{2013} Task with en-dash\n";
        let tasks = parse_tasks(content);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Task with en-dash");
    }

    // ── format_tasks tests ───────────────────────────────────────────

    #[test]
    fn format_tasks_empty() {
        let result = format_tasks(&[]);
        assert!(result.contains("No tasks"));
    }

    #[test]
    fn format_tasks_roundtrip() {
        let entries = vec![
            TaskEntry {
                id: 1,
                title: "First task".to_string(),
                status: TaskStatus::Pending,
            },
            TaskEntry {
                id: 2,
                title: "Second task".to_string(),
                status: TaskStatus::InProgress,
            },
            TaskEntry {
                id: 3,
                title: "Third task".to_string(),
                status: TaskStatus::Done,
            },
        ];

        let formatted = format_tasks(&entries);
        let parsed = parse_tasks(&formatted);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].id, 1);
        assert_eq!(parsed[0].title, "First task");
        assert_eq!(parsed[0].status, TaskStatus::Pending);
        assert_eq!(parsed[1].id, 2);
        assert_eq!(parsed[1].status, TaskStatus::InProgress);
        assert_eq!(parsed[2].id, 3);
        assert_eq!(parsed[2].status, TaskStatus::Done);
    }

    #[test]
    fn format_tasks_contains_header() {
        let entries = vec![TaskEntry {
            id: 1,
            title: "Task".to_string(),
            status: TaskStatus::Pending,
        }];
        let result = format_tasks(&entries);
        assert!(result.starts_with("# Tasks\n"));
    }

    // ── Tool metadata tests ──────────────────────────────────────────

    #[test]
    fn task_tools_have_correct_names() {
        use crate::Tool;
        assert_eq!(Tool::name(&TaskAddTool), "TaskAdd");
        assert_eq!(Tool::name(&TaskListTool), "TaskList");
        assert_eq!(Tool::name(&TaskUpdateTool), "TaskUpdate");
    }

    #[test]
    fn task_tools_schemas_are_valid_objects() {
        use crate::Tool;
        for tool in [
            TaskAddTool.input_schema(),
            TaskListTool.input_schema(),
            TaskUpdateTool.input_schema(),
        ] {
            assert_eq!(tool.get("type").and_then(|v| v.as_str()), Some("object"));
        }
    }
}
