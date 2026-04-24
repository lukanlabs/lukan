use std::path::PathBuf;

use async_trait::async_trait;
use base64::Engine;
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
        "ReadFiles"
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

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn search_hint(&self) -> Option<&str> {
        Some("read file contents with numbered lines")
    }

    fn activity_label(&self, _input: &serde_json::Value) -> Option<String> {
        Some("Reading file".to_string())
    }

    fn validate_input(&self, input: &serde_json::Value, ctx: &ToolContext) -> Result<(), String> {
        let file_path_str = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing required field: file_path".to_string())?;

        let path = PathBuf::from(file_path_str);
        let path = if path.is_absolute() {
            path
        } else {
            ctx.cwd.join(&path)
        };

        const BLOCKED_DEVICE_PATHS: &[&str] = &[
            "/dev/zero",
            "/dev/random",
            "/dev/urandom",
            "/dev/full",
            "/dev/stdin",
            "/dev/tty",
            "/dev/console",
            "/dev/stdout",
            "/dev/stderr",
            "/dev/fd/0",
            "/dev/fd/1",
            "/dev/fd/2",
        ];

        let path_str = path.to_string_lossy();
        if BLOCKED_DEVICE_PATHS
            .iter()
            .any(|blocked| path_str == *blocked)
            || (path_str.starts_with("/proc/")
                && (path_str.ends_with("/fd/0")
                    || path_str.ends_with("/fd/1")
                    || path_str.ends_with("/fd/2")))
        {
            return Err(format!(
                "Refusing to read special device path '{}'. Use a regular file path instead.",
                path.display()
            ));
        }

        if !path.exists() {
            return Err(format!(
                "Failed to read file: No such file or directory: {}",
                path.display()
            ));
        }

        Ok(())
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

        if let Err(msg) = ctx.check_path_allowed(&path) {
            return Ok(ToolResult::error(msg));
        }

        if let Err(msg) = ctx.check_sensitive(&path) {
            return Ok(ToolResult::error(msg));
        }

        let explicit_offset = input.get("offset").and_then(|v| v.as_u64());
        let offset = explicit_offset.unwrap_or(0);
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_LIMIT);

        // Check if the file is an image — return as base64 data URL
        let is_image = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                matches!(
                    e.to_lowercase().as_str(),
                    "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg"
                )
            })
            .unwrap_or(false);

        if is_image {
            match tokio::fs::read(&path).await {
                Ok(bytes) => {
                    let ext = path.extension().unwrap().to_str().unwrap().to_lowercase();
                    let mime = match ext.as_str() {
                        "png" => "image/png",
                        "jpg" | "jpeg" => "image/jpeg",
                        "gif" => "image/gif",
                        "webp" => "image/webp",
                        "bmp" => "image/bmp",
                        "svg" => "image/svg+xml",
                        _ => "image/png",
                    };
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let data_url = format!("data:{mime};base64,{b64}");
                    let mtime = tokio::fs::metadata(&path)
                        .await
                        .ok()
                        .and_then(|m| m.modified().ok());
                    ctx.read_files.lock().await.insert(path.clone(), mtime);
                    let size_kb = bytes.len() / 1024;
                    let mut result = ToolResult::success(format!(
                        "Image file: {} ({size_kb} KB, {mime})",
                        path.display()
                    ));
                    result.image = Some(data_url);
                    return Ok(result);
                }
                Err(e) => return Ok(ToolResult::error(format!("Failed to read image: {e}"))),
            }
        }

        // Check if we already have this file in context and it hasn't changed
        let current_mtime = tokio::fs::metadata(&path)
            .await
            .ok()
            .and_then(|m| m.modified().ok());
        {
            let read_map = ctx.read_files.lock().await;
            if let Some(prev_mtime) = read_map.get(&path) {
                // File was read before — check if it changed
                if *prev_mtime == current_mtime && explicit_offset.is_none() {
                    let total = tokio::fs::read_to_string(&path)
                        .await
                        .map(|c| c.lines().count())
                        .unwrap_or(0);
                    return Ok(ToolResult::success(format!(
                        "(file already in context, {} lines, not modified since last read)",
                        total
                    )));
                }
            }
        }

        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read file: {e}"))),
        };

        // Track that we've read this file with current mtime
        ctx.read_files
            .lock()
            .await
            .insert(path.clone(), current_mtime);

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
