use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use crate::subagent_worktrees::{
    WorktreeCleanupStatus, cleanup_stale_worktrees, load_records, remove_record, save_records,
};
use lukan_tools::{Tool, ToolContext};

pub struct SubagentWorktreeListTool;
pub struct SubagentWorktreeCleanupTool;

#[async_trait]
impl Tool for SubagentWorktreeListTool {
    fn name(&self) -> &str {
        "SubagentWorktreeList"
    }

    fn description(&self) -> &str {
        "List persisted subagent worktrees and their cleanup state."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let records = load_records(&ctx.cwd);
        if records.is_empty() {
            return Ok(ToolResult::success("No persisted subagent worktrees found."));
        }

        let mut out = format!("Found {} subagent worktree(s):\n", records.len());
        for record in records {
            out.push_str(&format!(
                "\n- {}\n  Task: {}\n  Isolation: {}\n  Worktree: {}\n  Branch: {}\n  Cleanup: {}\n",
                record.agent_id,
                record.task,
                record.isolation,
                record.worktree_path.display(),
                record.worktree_branch,
                record.cleanup_status.as_str(),
            ));
        }
        Ok(ToolResult::success(out))
    }
}

#[async_trait]
impl Tool for SubagentWorktreeCleanupTool {
    fn name(&self) -> &str {
        "SubagentWorktreeCleanup"
    }

    fn description(&self) -> &str {
        "Clean up stale or removable persisted subagent worktrees."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "olderThanSeconds": {
                    "type": "integer",
                    "description": "Only remove stale worktrees older than this many seconds (default: 86400)",
                    "default": 86400
                },
                "agentId": {
                    "type": "string",
                    "description": "Optional specific subagent id to forget after manual cleanup"
                }
            },
            "required": []
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        if let Some(agent_id) = input.get("agentId").and_then(|v| v.as_str()) {
            let mut records = load_records(&ctx.cwd);
            if let Some(record) = records.iter_mut().find(|r| r.agent_id == agent_id) {
                if record.cleanup_status == WorktreeCleanupStatus::Pending {
                    record.cleanup_status = WorktreeCleanupStatus::RemovedManual;
                    save_records(&ctx.cwd, &records)?;
                    return Ok(ToolResult::success(format!(
                        "Marked subagent worktree as manually removed: {agent_id}"
                    )));
                }
            }
            remove_record(&ctx.cwd, agent_id)?;
            return Ok(ToolResult::success(format!(
                "Removed persisted record for subagent worktree: {agent_id}"
            )));
        }

        let older_than = input
            .get("olderThanSeconds")
            .and_then(|v| v.as_i64())
            .unwrap_or(86_400);
        let removed = cleanup_stale_worktrees(&ctx.cwd, older_than)?;
        let status = if removed == 0 {
            "No stale subagent worktrees removed.".to_string()
        } else {
            format!("Removed {removed} stale subagent worktree(s).")
        };
        Ok(ToolResult::success(status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent_worktrees::{WorktreeRecord, save_records};
    use chrono::Utc;
    use std::path::PathBuf;

    #[tokio::test]
    async fn list_tool_reports_persisted_records() {
        let root = std::env::temp_dir().join("lukan-subagent-worktree-list-test");
        let _ = std::fs::remove_dir_all(&root);
        save_records(
            &root,
            &[WorktreeRecord {
                agent_id: "abc123".to_string(),
                task: "test task".to_string(),
                isolation: "worktree".to_string(),
                worktree_path: PathBuf::from("/tmp/wt"),
                worktree_branch: "lukan-subagent-abc123".to_string(),
                git_root: PathBuf::from("/tmp/repo"),
                head_commit: "deadbeef".to_string(),
                started_at: Utc::now(),
                completed_at: None,
                cleanup_status: WorktreeCleanupStatus::Pending,
            }],
        )
        .unwrap();

        let ctx = lukan_tools::ToolContext {
            progress_tx: None,
            event_tx: None,
            tool_call_id: None,
            read_files: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            cwd: root,
            bg_signal: None,
            sandbox: None,
            allowed_paths: None,
            cancel: None,
            session_id: None,
            extra_env: std::collections::HashMap::new(),
            agent_label: None,
            tab_id: None,
            blocked_env_vars: Vec::new(),
        };

        let result = SubagentWorktreeListTool.execute(json!({}), &ctx).await.unwrap();
        assert!(result.content.contains("abc123"));
        assert!(result.content.contains("worktree"));
    }

    #[tokio::test]
    async fn cleanup_tool_can_mark_specific_record_removed() {
        let root = std::env::temp_dir().join("lukan-subagent-worktree-cleanup-test");
        let _ = std::fs::remove_dir_all(&root);
        save_records(
            &root,
            &[WorktreeRecord {
                agent_id: "abc123".to_string(),
                task: "test task".to_string(),
                isolation: "worktree".to_string(),
                worktree_path: PathBuf::from("/tmp/wt"),
                worktree_branch: "lukan-subagent-abc123".to_string(),
                git_root: PathBuf::from("/tmp/repo"),
                head_commit: "deadbeef".to_string(),
                started_at: Utc::now(),
                completed_at: None,
                cleanup_status: WorktreeCleanupStatus::Pending,
            }],
        )
        .unwrap();

        let ctx = lukan_tools::ToolContext {
            progress_tx: None,
            event_tx: None,
            tool_call_id: None,
            read_files: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            cwd: root.clone(),
            bg_signal: None,
            sandbox: None,
            allowed_paths: None,
            cancel: None,
            session_id: None,
            extra_env: std::collections::HashMap::new(),
            agent_label: None,
            tab_id: None,
            blocked_env_vars: Vec::new(),
        };

        let result = SubagentWorktreeCleanupTool
            .execute(json!({"agentId": "abc123"}), &ctx)
            .await
            .unwrap();
        assert!(result.content.contains("abc123"));
        let records = load_records(&root);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].cleanup_status, WorktreeCleanupStatus::RemovedManual);
    }
}
