use std::path::PathBuf;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use similar::{ChangeTag, TextDiff};

use crate::{Tool, ToolContext};

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

        let path = PathBuf::from(file_path_str);
        if !path.is_absolute() {
            return Ok(ToolResult::error(format!(
                "Path must be absolute: {file_path_str}"
            )));
        }

        // Check if file exists and was read
        let old_content = if path.exists() {
            if !ctx.read_files.lock().await.contains(&path) {
                return Ok(ToolResult::error(format!(
                    "File exists but has not been read yet. Use ReadFile first: {file_path_str}"
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

        // Generate diff
        let old = old_content.as_deref().unwrap_or("");
        let diff = TextDiff::from_lines(old, content);
        let mut diff_str = String::new();
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            diff_str.push_str(&format!("{sign}{change}"));
        }

        let msg = if old_content.is_some() {
            format!("Updated {file_path_str}")
        } else {
            format!("Created {file_path_str}")
        };

        Ok(ToolResult::success(msg).with_diff(diff_str))
    }
}
