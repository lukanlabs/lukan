use std::path::PathBuf;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use similar::{ChangeTag, TextDiff};

use crate::{Tool, ToolContext};

pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str {
        "EditFile"
    }

    fn description(&self) -> &str {
        "Perform exact string replacements in files. The old_text must be unique unless replace_all is true."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "The exact text to find and replace"
                },
                "new_text": {
                    "type": "string",
                    "description": "The replacement text"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences (default: false)",
                    "default": false
                }
            },
            "required": ["file_path", "old_text", "new_text"]
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

        let old_text = input
            .get("old_text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: old_text"))?;

        let new_text = input
            .get("new_text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: new_text"))?;

        let replace_all = input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let path = PathBuf::from(file_path_str);
        if !path.is_absolute() {
            return Ok(ToolResult::error(format!(
                "Path must be absolute: {file_path_str}"
            )));
        }

        // Must have been read first
        if !ctx.read_files.lock().await.contains(&path) {
            return Ok(ToolResult::error(format!(
                "File has not been read yet. Use ReadFile first: {file_path_str}"
            )));
        }

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read file: {e}"))),
        };

        // Count occurrences
        let count = content.matches(old_text).count();

        if count == 0 {
            return Ok(ToolResult::error(format!(
                "old_text not found in {file_path_str}. Make sure it matches exactly (including whitespace)."
            )));
        }

        if !replace_all && count > 1 {
            return Ok(ToolResult::error(format!(
                "old_text found {count} times in {file_path_str}. Use replace_all: true or provide more context to make it unique."
            )));
        }

        let new_content = if replace_all {
            content.replace(old_text, new_text)
        } else {
            content.replacen(old_text, new_text, 1)
        };

        // Write the file
        tokio::fs::write(&path, &new_content).await?;

        // Generate diff
        let diff = TextDiff::from_lines(&content, &new_content);
        let mut diff_str = String::new();
        for change in diff.iter_all_changes() {
            let sign = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            diff_str.push_str(&format!("{sign}{change}"));
        }

        let msg = if replace_all {
            format!("Replaced {count} occurrences in {file_path_str}")
        } else {
            format!("Edited {file_path_str}")
        };

        Ok(ToolResult::success(msg).with_diff(diff_str))
    }
}
