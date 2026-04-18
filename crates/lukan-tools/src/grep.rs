use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkMatch};
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use crate::{Tool, ToolContext, sandbox};

const MAX_OUTPUT_BYTES: usize = 30_000;
const MAX_LINE_LEN: usize = 500;

pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "Search file contents using regex patterns. Fast, respects .gitignore."
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
                    "description": "Output format (default: \"files_with_matches\")",
                    "default": "files_with_matches"
                },
                "type": {
                    "type": "string",
                    "description": "File type filter (e.g. \"js\", \"py\", \"rust\")"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case insensitive search (default: false)",
                    "default": false
                },
                "multiline": {
                    "type": "boolean",
                    "description": "Enable multiline matching (default: false)",
                    "default": false
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches (default: 50)",
                    "default": 50
                },
                "context_lines": {
                    "type": "integer",
                    "description": "Context lines around each match"
                },
                "before_context": {
                    "type": "integer",
                    "description": "Lines before each match"
                },
                "after_context": {
                    "type": "integer",
                    "description": "Lines after each match"
                },
                "head_limit": {
                    "type": "integer",
                    "description": "Limit output to first N lines (0 = unlimited)",
                    "default": 0
                }
            },
            "required": ["pattern"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn search_hint(&self) -> Option<&str> {
        Some("search file contents by regex")
    }

    fn activity_label(&self, _input: &serde_json::Value) -> Option<String> {
        Some("Searching files".to_string())
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
            .unwrap_or(50) as usize;
        let context_before = input
            .get("before_context")
            .and_then(|v| v.as_u64())
            .or_else(|| input.get("context_lines").and_then(|v| v.as_u64()))
            .unwrap_or(0) as usize;
        let context_after = input
            .get("after_context")
            .and_then(|v| v.as_u64())
            .or_else(|| input.get("context_lines").and_then(|v| v.as_u64()))
            .unwrap_or(0) as usize;
        let head_limit = input
            .get("head_limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;

        // Resolve path
        let resolved_path = {
            let p = std::path::PathBuf::from(path);
            if p.is_absolute() { p } else { ctx.cwd.join(&p) }
        };
        if let Err(msg) = ctx.check_path_allowed(&resolved_path) {
            return Ok(ToolResult::error(msg));
        }
        if resolved_path.is_file()
            && let Err(msg) = ctx.check_sensitive(&resolved_path)
        {
            return Ok(ToolResult::error(msg));
        }

        // Sensitive patterns
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

        // Build matcher (ripgrep's regex engine)
        let matcher = RegexMatcherBuilder::new()
            .case_insensitive(case_insensitive)
            .multi_line(multiline)
            .dot_matches_new_line(multiline)
            .build(pattern)
            .map_err(|e| anyhow::anyhow!("Invalid regex: {e}"))?;

        let search_path = resolved_path.clone();
        let output_mode_owned = output_mode.to_string();
        let file_type_owned = file_type.map(|s| s.to_string());
        let glob_owned = glob_pattern.map(|s| s.to_string());
        let cancel = ctx.cancel.clone();

        let result = tokio::task::spawn_blocking(move || {
            // Build walker (respects .gitignore, parallel)
            let mut builder = ignore::WalkBuilder::new(&search_path);
            builder
                .hidden(false)
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true);

            if let Some(ref ft) = file_type_owned {
                let mut tb = ignore::types::TypesBuilder::new();
                tb.add_defaults();
                tb.select(ft);
                if let Ok(types) = tb.build() {
                    builder.types(types);
                }
            }

            if let Some(ref glob) = glob_owned {
                let mut ob = ignore::overrides::OverrideBuilder::new(&search_path);
                let _ = ob.add(glob);
                if let Ok(ov) = ob.build() {
                    builder.overrides(ov);
                }
            }

            let results: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let count = Arc::new(AtomicUsize::new(0));

            // Use parallel walker for speed
            let walker = builder.build_parallel();
            walker.run(|| {
                let matcher = matcher.clone();
                let results = Arc::clone(&results);
                let count = Arc::clone(&count);
                let sensitive = sensitive_patterns.clone();
                let mode = output_mode_owned.clone();
                let cancel = cancel.clone();

                Box::new(move |entry| {
                    // Check cancellation
                    if let Some(ref token) = cancel
                        && token.is_cancelled()
                    {
                        return ignore::WalkState::Quit;
                    }

                    // Check limit
                    if count.load(Ordering::Relaxed) >= max_results {
                        return ignore::WalkState::Quit;
                    }

                    let entry = match entry {
                        Ok(e) => e,
                        Err(_) => return ignore::WalkState::Continue,
                    };

                    let path = entry.path();
                    if !path.is_file() {
                        return ignore::WalkState::Continue;
                    }

                    // Skip sensitive
                    let pat_refs: Vec<&str> = sensitive.iter().map(|s| s.as_str()).collect();
                    if sandbox::match_sensitive_pattern(path, &pat_refs).is_some() {
                        return ignore::WalkState::Continue;
                    }

                    let display_path = path.to_string_lossy().to_string();

                    match mode.as_str() {
                        "files_with_matches" => {
                            // Just check if file matches — no need to collect lines
                            let mut found = false;
                            let mut searcher = SearcherBuilder::new().build();
                            let _ = searcher.search_path(&matcher, path, MatchSink(&mut found));
                            if found {
                                count.fetch_add(1, Ordering::Relaxed);
                                results.lock().unwrap().push(display_path);
                            }
                        }
                        "count" => {
                            let mut file_count = 0usize;
                            let mut searcher = SearcherBuilder::new().build();
                            let _ =
                                searcher.search_path(&matcher, path, CountSink(&mut file_count));
                            if file_count > 0 {
                                count.fetch_add(1, Ordering::Relaxed);
                                results
                                    .lock()
                                    .unwrap()
                                    .push(format!("{display_path}:{file_count}"));
                            }
                        }
                        _ => {
                            // Content mode
                            let mut lines = Vec::new();
                            let mut searcher = SearcherBuilder::new()
                                .before_context(context_before)
                                .after_context(context_after)
                                .build();
                            let _ = searcher.search_path(
                                &matcher,
                                path,
                                ContentSink {
                                    path: &display_path,
                                    lines: &mut lines,
                                },
                            );
                            if !lines.is_empty() {
                                let n = lines.len();
                                count.fetch_add(n, Ordering::Relaxed);
                                results.lock().unwrap().extend(lines);
                            }
                        }
                    }

                    ignore::WalkState::Continue
                })
            });

            Arc::try_unwrap(results).unwrap().into_inner().unwrap()
        })
        .await?;

        let mut text = result.join("\n");

        // Truncate long lines
        text = text
            .lines()
            .map(|l| {
                if l.len() > MAX_LINE_LEN {
                    format!("{}...", &l[..l.floor_char_boundary(MAX_LINE_LEN)])
                } else {
                    l.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Limit total lines
        let limit = if head_limit > 0 {
            head_limit
        } else {
            max_results
        };
        let total = text.lines().count();
        if total > limit {
            text = text.lines().take(limit).collect::<Vec<_>>().join("\n");
            text.push_str(&format!("\n... ({} more results)", total - limit));
        }

        if text.len() > MAX_OUTPUT_BYTES {
            text.truncate(text.floor_char_boundary(MAX_OUTPUT_BYTES));
            text.push_str("\n... (output truncated)");
        }

        if text.is_empty() {
            Ok(ToolResult::success("No matches found."))
        } else {
            Ok(ToolResult::success(text))
        }
    }
}

// ── Sinks for grep-searcher ─────────────────────────────────────────

/// Sink that just checks if there's any match (for files_with_matches mode).
struct MatchSink<'a>(&'a mut bool);

impl Sink for MatchSink<'_> {
    type Error = std::io::Error;
    fn matched(&mut self, _: &Searcher, _: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        *self.0 = true;
        Ok(false) // stop after first match
    }
}

/// Sink that counts matches.
struct CountSink<'a>(&'a mut usize);

impl Sink for CountSink<'_> {
    type Error = std::io::Error;
    fn matched(&mut self, _: &Searcher, _: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        *self.0 += 1;
        Ok(true)
    }
}

/// Sink that collects matching lines with path and line number.
struct ContentSink<'a> {
    path: &'a str,
    lines: &'a mut Vec<String>,
}

impl Sink for ContentSink<'_> {
    type Error = std::io::Error;
    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        let line_num = mat.line_number().unwrap_or(0);
        let text = String::from_utf8_lossy(mat.bytes()).trim_end().to_string();
        self.lines
            .push(format!("{}:{}:{}", self.path, line_num, text));
        Ok(true)
    }

    fn context(
        &mut self,
        _: &Searcher,
        ctx: &grep_searcher::SinkContext<'_>,
    ) -> Result<bool, Self::Error> {
        let line_num = ctx.line_number().unwrap_or(0);
        let text = String::from_utf8_lossy(ctx.bytes()).trim_end().to_string();
        self.lines
            .push(format!("{}:{}-{}", self.path, line_num, text));
        Ok(true)
    }
}
