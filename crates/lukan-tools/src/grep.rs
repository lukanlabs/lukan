use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use tokio::process::Command;

use crate::{Tool, ToolContext, sandbox};

const MAX_OUTPUT_BYTES: usize = 30_000;
const GREP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

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
                "output_mode": {
                    "type": "string",
                    "enum": ["content", "files_with_matches", "count"],
                    "description": "Output format: \"content\" shows matching lines, \"files_with_matches\" shows file paths, \"count\" shows match counts (default: \"files_with_matches\")",
                    "default": "files_with_matches"
                },
                "type": {
                    "type": "string",
                    "description": "File type filter (e.g. \"js\", \"py\", \"rust\"). Maps to rg --type."
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case insensitive search (default: false)",
                    "default": false
                },
                "multiline": {
                    "type": "boolean",
                    "description": "Enable multiline matching where patterns can span lines (default: false)",
                    "default": false
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches (default: 50)",
                    "default": 50
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Number of context lines around each match (content mode only)"
                },
                "before_context": {
                    "type": "integer",
                    "description": "Number of lines to show before each match (content mode only)"
                },
                "after_context": {
                    "type": "integer",
                    "description": "Number of lines to show after each match (content mode only)"
                },
                "head_limit": {
                    "type": "integer",
                    "description": "Limit output to first N lines/entries. 0 = unlimited (default: 0)",
                    "default": 0
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

        // If targeting a single file, check sensitive patterns
        if resolved_path.is_file()
            && let Err(msg) = ctx.check_sensitive(&resolved_path)
        {
            return Ok(ToolResult::error(msg));
        }

        // Build sensitive patterns for exclusion in rg/grep
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

        let glob_pattern = input.get("glob").and_then(|v| v.as_str());
        let output_mode = input
            .get("output_mode")
            .and_then(|v| v.as_str())
            .unwrap_or("files_with_matches");
        let file_type = input.get("type").and_then(|v| v.as_str());
        let case_insensitive = input
            .get("case_insensitive")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let multiline = input
            .get("multiline")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(50);
        let context_lines = input.get("context_lines").and_then(|v| v.as_u64());
        let before_context = input.get("before_context").and_then(|v| v.as_u64());
        let after_context = input.get("after_context").and_then(|v| v.as_u64());
        let head_limit = input
            .get("head_limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        let opts = GrepOpts {
            pattern,
            path,
            glob_pattern,
            output_mode,
            file_type,
            case_insensitive,
            multiline,
            max_results,
            context_lines,
            before_context,
            after_context,
            sensitive_patterns: &sensitive_patterns,
        };

        // Try rg first, fallback to grep
        let output = try_rg(&opts, &ctx.cwd)
            .await
            .map_err(|_| anyhow::anyhow!("rg not available"));

        let output = match output {
            Ok(o) => o,
            Err(_) => try_grep(&opts, &ctx.cwd).await?,
        };

        let mut text = String::from_utf8_lossy(&output.stdout).to_string();

        // Apply head_limit by truncating to first N lines
        if head_limit > 0 {
            let truncated: String = text.lines().take(head_limit).collect::<Vec<_>>().join("\n");
            let was_truncated = text.lines().count() > head_limit;
            text = truncated;
            if was_truncated {
                text.push_str("\n... (head_limit reached)");
            }
        }

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

struct GrepOpts<'a> {
    pattern: &'a str,
    path: &'a str,
    glob_pattern: Option<&'a str>,
    output_mode: &'a str,
    file_type: Option<&'a str>,
    case_insensitive: bool,
    multiline: bool,
    max_results: u64,
    context_lines: Option<u64>,
    before_context: Option<u64>,
    after_context: Option<u64>,
    sensitive_patterns: &'a [String],
}

async fn try_rg(
    opts: &GrepOpts<'_>,
    cwd: &std::path::Path,
) -> anyhow::Result<std::process::Output> {
    let mut cmd = Command::new("rg");

    match opts.output_mode {
        "files_with_matches" => {
            cmd.arg("-l");
        }
        "count" => {
            cmd.arg("-c");
        }
        _ => {
            // "content" mode — show line numbers and respect max-count
            cmd.arg("-n");
            cmd.arg("--max-count").arg(opts.max_results.to_string());
            if let Some(b) = opts.before_context {
                cmd.arg("-B").arg(b.to_string());
            }
            if let Some(a) = opts.after_context {
                cmd.arg("-A").arg(a.to_string());
            }
            if let Some(ctx) = opts.context_lines {
                cmd.arg("-C").arg(ctx.to_string());
            }
        }
    }

    if opts.case_insensitive {
        cmd.arg("-i");
    }
    if opts.multiline {
        cmd.arg("-U").arg("--multiline-dotall");
    }
    if let Some(ft) = opts.file_type {
        cmd.arg("--type").arg(ft);
    }
    if let Some(glob) = opts.glob_pattern {
        cmd.arg("--glob").arg(glob);
    }

    // Exclude sensitive patterns (gitignore-style)
    for sp in opts.sensitive_patterns {
        if let Some(dir) = sp.strip_suffix('/') {
            // Directory pattern: exclude dir/**
            cmd.arg("--glob").arg(format!("!{dir}/**"));
        } else {
            cmd.arg("--glob").arg(format!("!{sp}"));
        }
    }

    cmd.arg(opts.pattern).arg(opts.path).current_dir(cwd);

    let output = tokio::time::timeout(GREP_TIMEOUT, cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("rg timed out after {}s", GREP_TIMEOUT.as_secs()))??;
    // rg exit code 1 means no matches (not an error)
    Ok(output)
}

async fn try_grep(
    opts: &GrepOpts<'_>,
    cwd: &std::path::Path,
) -> anyhow::Result<std::process::Output> {
    let mut cmd = Command::new("grep");

    match opts.output_mode {
        "files_with_matches" => {
            cmd.arg("-rl");
        }
        "count" => {
            cmd.arg("-rc");
        }
        _ => {
            // "content" mode
            cmd.arg("-rn");
            cmd.arg("--max-count").arg(opts.max_results.to_string());
            if let Some(b) = opts.before_context {
                cmd.arg("-B").arg(b.to_string());
            }
            if let Some(a) = opts.after_context {
                cmd.arg("-A").arg(a.to_string());
            }
            if let Some(ctx) = opts.context_lines {
                cmd.arg("-C").arg(ctx.to_string());
            }
        }
    }

    if opts.case_insensitive {
        cmd.arg("-i");
    }

    // Exclude sensitive patterns (gitignore-style)
    // Note: --type and --multiline are not supported in grep fallback
    for sp in opts.sensitive_patterns {
        if let Some(dir) = sp.strip_suffix('/') {
            cmd.arg("--exclude-dir").arg(dir);
        } else {
            cmd.arg("--exclude").arg(sp);
        }
    }

    cmd.arg(opts.pattern).arg(opts.path).current_dir(cwd);

    let output = tokio::time::timeout(GREP_TIMEOUT, cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("grep timed out after {}s", GREP_TIMEOUT.as_secs()))??;
    Ok(output)
}
