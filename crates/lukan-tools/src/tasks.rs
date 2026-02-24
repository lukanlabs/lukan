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
