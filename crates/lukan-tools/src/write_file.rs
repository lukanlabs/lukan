use std::path::PathBuf;

use async_trait::async_trait;
use lukan_core::models::checkpoints::{FileOperation, FileSnapshot};
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use similar::{ChangeTag, TextDiff};

use crate::{Tool, ToolContext, format_stats};

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "WriteFile"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if needed. You must read existing files before overwriting them."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let file_path_str = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: file_path"))?;

        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: content"))?;

        if content.is_empty() {
            return Ok(ToolResult::error(
                "Content is empty. If conversation was compacted, re-generate the file content before writing.",
            ));
        }

        let path = PathBuf::from(file_path_str);
        let path = if path.is_absolute() {
            path
        } else {
            ctx.cwd.join(&path)
        };

        if let Err(msg) = ctx.check_path_allowed(&path) {
            return Ok(ToolResult::error(msg));
        }

        if let Err(msg) = ctx.check_sensitive(&path) {
            return Ok(ToolResult::error(msg));
        }

        // Check if file exists and was read
        let old_content = if path.exists() {
            if !ctx.read_files.lock().await.contains(&path) {
                return Ok(ToolResult::error(format!(
                    "File exists but has not been read yet. Use ReadFiles first: {file_path_str}"
                )));
            }
            tokio::fs::read_to_string(&path).await.ok()
        } else {
            None
        };

        // Create parent directories
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Write the file
        tokio::fs::write(&path, content).await?;

        // Track as read for future edits
        ctx.read_files.lock().await.insert(path);

        // Generate diff — 3 lines of context around each change (like git)
        let old = old_content.as_deref().unwrap_or("");
        let diff = TextDiff::from_lines(old, content);
        let mut diff_str = format!("--- {file_path_str}\n");
        let mut added = 0usize;
        let mut removed = 0usize;
        for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
            diff_str.push_str(&format!("{}\n", hunk.header()));
            for change in hunk.iter_changes() {
                let sign = match change.tag() {
                    ChangeTag::Delete => {
                        removed += 1;
                        "-"
                    }
                    ChangeTag::Insert => {
                        added += 1;
                        "+"
                    }
                    ChangeTag::Equal => " ",
                };
                diff_str.push_str(&format!("{sign}{}", change.value()));
            }
        }

        let operation = if old_content.is_some() {
            FileOperation::Modified
        } else {
            FileOperation::Created
        };
        let stats = format_stats(added, removed);
        let msg = stats;

        let snapshot = FileSnapshot {
            path: file_path_str.to_string(),
            operation,
            before: old_content,
            after: Some(content.to_string()),
            diff: Some(diff_str.clone()),
            additions: added as u32,
            deletions: removed as u32,
        };

        Ok(ToolResult::success(msg)
            .with_diff(diff_str)
            .with_snapshot(snapshot))
    }
}
