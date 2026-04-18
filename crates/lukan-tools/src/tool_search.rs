use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use crate::{Tool, ToolContext, ToolRegistry};

pub struct ToolSearchTool;

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        "ToolSearch"
    }

    fn description(&self) -> &str {
        "Search available deferred tools by name and metadata. Use this when you need a specialized tool that may not be visible in the default tool list."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Keywords to find a deferred tool by name or purpose"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches to return (default: 5)",
                    "default": 5
                }
            },
            "required": ["query"]
        })
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn search_hint(&self) -> Option<&str> {
        Some("find a specialized deferred tool")
    }

    fn activity_label(&self, _input: &serde_json::Value) -> Option<String> {
        Some("Searching tools".to_string())
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: query"))?
            .trim()
            .to_lowercase();
        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;

        let mut registry = crate::create_default_registry();
        registry.register(Box::new(ToolSearchTool));
        let results = search_deferred_tools(&registry, &query, max_results);

        if results.is_empty() {
            return Ok(ToolResult::success("No matching deferred tools found."));
        }

        let mut output = format!("Found {} deferred tool(s):\n", results.len());
        for tool in results {
            output.push_str(&format!(
                "\n- {}\n  Description: {}{}\n",
                tool.name,
                tool.description,
                tool.search_hint
                    .as_deref()
                    .map(|h| format!("\n  Search hint: {h}"))
                    .unwrap_or_default()
            ));
        }

        Ok(ToolResult::success(output))
    }
}

#[derive(Debug, Clone)]
pub struct ToolSearchResult {
    pub name: String,
    pub description: String,
    pub search_hint: Option<String>,
    score: usize,
}

fn parse_tool_name(name: &str) -> (Vec<String>, String) {
    if let Some(without_prefix) = name.strip_prefix("mcp__") {
        let normalized = without_prefix.to_lowercase();
        let parts = normalized
            .split("__")
            .flat_map(|p| p.split('_'))
            .filter(|p| !p.is_empty())
            .map(|s| s.to_string())
            .collect::<Vec<_>>();
        let full = normalized.replace("__", " ").replace('_', " ");
        return (parts, full);
    }

    let spaced = name
        .replace(|c: char| c == '_', " ")
        .chars()
        .enumerate()
        .fold(String::new(), |mut acc, (idx, ch)| {
            if idx > 0 && ch.is_uppercase() {
                acc.push(' ');
            }
            acc.push(ch);
            acc
        })
        .to_lowercase();

    let parts = spaced
        .split_whitespace()
        .filter(|p| !p.is_empty())
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    let full = parts.join(" ");
    (parts, full)
}

pub fn search_deferred_tools(
    registry: &ToolRegistry,
    query: &str,
    max_results: usize,
) -> Vec<ToolSearchResult> {
    let query_normalized = query.trim().to_lowercase();
    let terms: Vec<String> = query_normalized
        .split_whitespace()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    if terms.is_empty() {
        return Vec::new();
    }

    let deferred_tools: Vec<_> = registry.tools.values().filter(|tool| tool.is_deferred()).collect();

    if let Some(exact) = deferred_tools.iter().find(|tool| tool.name().eq_ignore_ascii_case(&query_normalized)) {
        return vec![ToolSearchResult {
            name: exact.name().to_string(),
            description: exact.description().to_string(),
            search_hint: exact.search_hint().map(|s| s.to_string()),
            score: usize::MAX,
        }];
    }

    if query_normalized.starts_with("mcp__") {
        let prefix_matches: Vec<ToolSearchResult> = deferred_tools
            .iter()
            .filter(|tool| tool.name().to_lowercase().starts_with(&query_normalized))
            .take(max_results)
            .map(|tool| ToolSearchResult {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                search_hint: tool.search_hint().map(|s| s.to_string()),
                score: usize::MAX - 1,
            })
            .collect();
        if !prefix_matches.is_empty() {
            return prefix_matches;
        }
    }

    let mut results: Vec<ToolSearchResult> = deferred_tools
        .into_iter()
        .filter_map(|tool| {
            let name = tool.name().to_string();
            let description = tool.description().to_string();
            let search_hint = tool.search_hint().map(|s| s.to_string());
            let (name_parts, name_full) = parse_tool_name(&name);
            let description_lower = description.to_lowercase();
            let hint_lower = search_hint.clone().unwrap_or_default().to_lowercase();

            let mut score = 0usize;

            if name_full == query_normalized {
                score += 1000;
            }
            if name.to_lowercase() == query_normalized {
                score += 1000;
            }
            if name_full.contains(&query_normalized) {
                score += 50;
            }
            if hint_lower.contains(&query_normalized) {
                score += 30;
            }
            if description_lower.contains(&query_normalized) {
                score += 20;
            }

            for term in &terms {
                if name_parts.iter().any(|p| p == term) {
                    score += 20;
                }
                if name_full.contains(term) {
                    score += 10;
                }
                if hint_lower.contains(term) {
                    score += 8;
                }
                if description_lower.contains(term) {
                    score += 4;
                }
            }

            if score == 0 {
                return None;
            }

            Some(ToolSearchResult {
                name,
                description,
                search_hint,
                score,
            })
        })
        .collect();

    results.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
    results.truncate(max_results);
    results
}
