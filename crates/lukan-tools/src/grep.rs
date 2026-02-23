use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use tokio::process::Command;

use crate::{Tool, ToolContext};

const MAX_OUTPUT_BYTES: usize = 30_000;

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "Search file contents using regex patterns. Uses ripgrep (rg) with grep fallback."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in (default: current dir)",
                    "default": "."
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g. \"*.rs\", \"*.{ts,tsx}\")"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case insensitive search (default: false)",
                    "default": false
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches (default: 50)",
                    "default": 50
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines around each match"
                }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: pattern"))?;

        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");

        // Check path restrictions
        let resolved_path = {
            let p = std::path::PathBuf::from(path);
            if p.is_absolute() { p } else { ctx.cwd.join(&p) }
        };
        if let Err(msg) = ctx.check_path_allowed(&resolved_path) {
            return Ok(ToolResult::error(msg));
        }

        let glob_pattern = input.get("glob").and_then(|v| v.as_str());
        let case_insensitive = input
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(50);
        let context_lines = input.get("context_lines").and_then(|v| v.as_u64());

        // Try rg first, fallback to grep
        let output = try_rg(
            pattern,
            path,
            glob_pattern,
            case_insensitive,
            max_results,
            context_lines,
            &ctx.cwd,
        )
        .await
        .map_err(|_| anyhow::anyhow!("rg not available"));

        let output = match output {
            Ok(o) => o,
            Err(_) => try_grep(pattern, path, case_insensitive, max_results, &ctx.cwd).await?,
        };

        let mut text = String::from_utf8_lossy(&output.stdout).to_string();
        if text.len() > MAX_OUTPUT_BYTES {
            text.truncate(MAX_OUTPUT_BYTES);
            text.push_str("\n... (output truncated)");
        }

        if text.is_empty() {
            Ok(ToolResult::success("No matches found."))
        } else {
            Ok(ToolResult::success(text))
        }
    }
}

async fn try_rg(
    pattern: &str,
    path: &str,
    glob_pattern: Option<&str>,
    case_insensitive: bool,
    max_results: u64,
    context_lines: Option<u64>,
    cwd: &std::path::Path,
) -> anyhow::Result<std::process::Output> {
    let mut cmd = Command::new("rg");
    cmd.arg("-n"); // line numbers
    cmd.arg("--max-count").arg(max_results.to_string());

    if case_insensitive {
        cmd.arg("-i");
    }
    if let Some(ctx) = context_lines {
        cmd.arg("-C").arg(ctx.to_string());
    }
    if let Some(glob) = glob_pattern {
        cmd.arg("--glob").arg(glob);
    }

    cmd.arg(pattern).arg(path).current_dir(cwd);

    let output = cmd.output().await?;
    // rg exit code 1 means no matches (not an error)
    Ok(output)
}

async fn try_grep(
    pattern: &str,
    path: &str,
    case_insensitive: bool,
    max_results: u64,
    cwd: &std::path::Path,
) -> anyhow::Result<std::process::Output> {
    let mut cmd = Command::new("grep");
    cmd.arg("-rn");
    cmd.arg("--max-count").arg(max_results.to_string());

    if case_insensitive {
        cmd.arg("-i");
    }

    cmd.arg(pattern).arg(path).current_dir(cwd);

    Ok(cmd.output().await?)
}
