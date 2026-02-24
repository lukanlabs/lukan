use std::path::PathBuf;

use async_trait::async_trait;
use globset::Glob;
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use walkdir::WalkDir;

use crate::{Tool, ToolContext, sandbox};

const DEFAULT_MAX_RESULTS: u64 = 100;

pub struct GlobTool;

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "Glob"
    }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. Returns paths sorted by modification time (newest first)."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match (e.g. \"**/*.rs\", \"src/**/*.ts\")"
                },
                "path": {
                    "type": "string",
                    "description": "Base directory to search in (default: current dir)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 100)",
                    "default": 100
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
        let pattern_str = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: pattern"))?;

        let base_path = input
            .get("path")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
            .unwrap_or_else(|| ctx.cwd.clone());

        let base_path = if base_path.is_absolute() {
            base_path
        } else {
            ctx.cwd.join(&base_path)
        };

        if let Err(msg) = ctx.check_path_allowed(&base_path) {
            return Ok(ToolResult::error(msg));
        }

        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_RESULTS) as usize;

        let glob = match Glob::new(pattern_str) {
            Ok(g) => g.compile_matcher(),
            Err(e) => return Ok(ToolResult::error(format!("Invalid glob pattern: {e}"))),
        };

        // Extract sensitive patterns for use inside spawn_blocking
        let sensitive_patterns: Vec<String> = if let Some(ref sb) = ctx.sandbox {
            if sb.sensitive_patterns.is_empty() {
                sandbox::DEFAULT_SENSITIVE_PATTERNS
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect()
            } else {
                sb.sensitive_patterns.clone()
            }
        } else {
            sandbox::DEFAULT_SENSITIVE_PATTERNS
                .iter()
                .map(|s| (*s).to_string())
                .collect()
        };

        // Run blocking walk in a spawn_blocking to avoid blocking the async runtime
        let base = base_path.clone();
        let matches = tokio::task::spawn_blocking(move || {
            let mut results: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
            for entry in WalkDir::new(&base)
                .follow_links(true)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();

                // Skip files/dirs matching sensitive patterns (gitignore-style)
                let pat_refs: Vec<&str> = sensitive_patterns.iter().map(|s| s.as_str()).collect();
                if sandbox::match_sensitive_pattern(path, &pat_refs).is_some() {
                    continue;
                }

                // Match against relative path from base
                if let Ok(rel) = path.strip_prefix(&base)
                    && glob.is_match(rel)
                {
                    let mtime = entry
                        .metadata()
                        .ok()
                        .and_then(|m| m.modified().ok())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    results.push((path.to_path_buf(), mtime));
                }
            }
            // Sort by mtime descending (newest first)
            results.sort_by(|a, b| b.1.cmp(&a.1));
            results
        })
        .await?;

        if matches.is_empty() {
            return Ok(ToolResult::success("No files matched."));
        }

        let total = matches.len();
        let truncated = total > max_results;
        let displayed: Vec<String> = matches
            .into_iter()
            .take(max_results)
            .map(|(p, _)| p.display().to_string())
            .collect();

        let mut result = displayed.join("\n");
        if truncated {
            result.push_str(&format!(
                "\n\n... ({total} total matches, showing {max_results})"
            ));
        }

        Ok(ToolResult::success(result))
    }
}
