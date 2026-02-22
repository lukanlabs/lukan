use std::path::PathBuf;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use crate::{Tool, ToolContext};

const DEFAULT_LIMIT: u64 = 2000;
const MAX_LINE_LEN: usize = 2000;
const BG_LOG_TAIL_LINES: usize = 50;

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "ReadFile"
    }

    fn description(&self) -> &str {
        "Read a file from the filesystem. Returns numbered lines. Use absolute paths."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-based)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read (default: 2000)",
                    "default": 2000
                }
            },
            "required": ["file_path"]
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

        let path = PathBuf::from(file_path_str);
        let path = if path.is_absolute() {
            path
        } else {
            ctx.cwd.join(&path)
        };

        let explicit_offset = input.get("offset").and_then(|v| v.as_u64());
        let offset = explicit_offset.unwrap_or(0);
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_LIMIT);

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read file: {e}"))),
        };

        // Track that we've read this file
        ctx.read_files.lock().await.insert(path.clone());

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // Auto-tail background process log files: show last N lines when no
        // explicit offset is given (matches Node.js kite-agent behavior)
        let is_bg_log = path
            .file_name()
            .and_then(|f| f.to_str())
            .map(|f| f.starts_with("lukan-bg-") && f.ends_with(".log"))
            .unwrap_or(false);

        let start = if is_bg_log && explicit_offset.is_none() && total_lines > BG_LOG_TAIL_LINES {
            total_lines - BG_LOG_TAIL_LINES
        } else if offset > 0 {
            (offset as usize).saturating_sub(1)
        } else {
            0
        };
        let end = (start + limit as usize).min(total_lines);

        let mut result = String::new();
        for (idx, line) in lines[start..end].iter().enumerate() {
            let line_num = start + idx + 1;
            let display_line = if line.len() > MAX_LINE_LEN {
                format!("{}... (truncated)", &line[..MAX_LINE_LEN])
            } else {
                line.to_string()
            };
            result.push_str(&format!("{line_num:>5}\t{display_line}\n"));
        }

        // Prepend header when auto-tailing a bg log
        if is_bg_log && explicit_offset.is_none() && start > 0 {
            let header = format!(
                "(showing last {} of {} lines — background process log)\n\n",
                total_lines - start,
                total_lines
            );
            result.insert_str(0, &header);
        }

        if result.is_empty() {
            result = "(empty file)".to_string();
        } else if end < total_lines {
            result.push_str(&format!(
                "\n... ({} more lines, {} total)",
                total_lines - end,
                total_lines
            ));
        }

        Ok(ToolResult::success(result))
    }
}
