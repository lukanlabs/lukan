use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use regex::Regex;
use serde_json::json;

use crate::{Tool, ToolContext, ToolRegistry};

#[derive(Debug, Clone)]
pub struct ToolSearchResult {
    pub name: String,
    pub description: String,
    pub search_hint: Option<String>,
    pub source: Option<String>,
    score: usize,
}

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

        let registry = crate::create_default_registry();
        let results = search_deferred_tools(&registry, &query, max_results);

        if results.is_empty() {
            return Ok(ToolResult::success("No matching deferred tools found."));
        }

        let mut output = format!("Found {} deferred tool(s):\n", results.len());
        for tool in results {
            output.push_str(&format!(
                "\n- {}\n  Description: {}{}{}\n",
                tool.name,
                tool.description,
                tool.search_hint
                    .as_deref()
                    .map(|h| format!("\n  Search hint: {h}"))
                    .unwrap_or_default(),
                tool.source
                    .as_deref()
                    .map(|s| format!("\n  Source: {s}"))
                    .unwrap_or_default()
            ));
        }

        Ok(ToolResult::success(output))
    }
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
        .replace('_', " ")
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

fn compile_term_patterns(terms: &[String]) -> Vec<(String, Regex)> {
    terms
        .iter()
        .filter_map(|term| {
            Regex::new(&format!(r"\b{}\b", regex::escape(term)))
                .ok()
                .map(|re| (term.clone(), re))
        })
        .collect()
}

fn build_result(tool: &dyn Tool, score: usize) -> ToolSearchResult {
    ToolSearchResult {
        name: tool.name().to_string(),
        description: tool.description().to_string(),
        search_hint: tool.search_hint().map(|s| s.to_string()),
        source: tool.source().map(|s| s.to_string()),
        score,
    }
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

    let deferred_tools: Vec<_> = registry
        .tools
        .values()
        .filter(|tool| tool.is_deferred())
        .collect();

    if let Some(exact) = deferred_tools
        .iter()
        .find(|tool| tool.name().eq_ignore_ascii_case(&query_normalized))
    {
        return vec![build_result(exact.as_ref(), usize::MAX)];
    }

    if let Some(select_query) = query_normalized.strip_prefix("select:") {
        let requested: Vec<String> = select_query
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let mut found = Vec::new();
        for name in requested {
            if let Some(tool) = deferred_tools
                .iter()
                .find(|tool| tool.name().eq_ignore_ascii_case(&name))
            {
                found.push(build_result(tool.as_ref(), usize::MAX - 1));
            }
        }
        if !found.is_empty() {
            return found;
        }
    }

    if query_normalized.starts_with("mcp__") {
        let prefix_matches: Vec<ToolSearchResult> = deferred_tools
            .iter()
            .filter(|tool| tool.name().to_lowercase().starts_with(&query_normalized))
            .take(max_results)
            .map(|tool| build_result(tool.as_ref(), usize::MAX - 2))
            .collect();
        if !prefix_matches.is_empty() {
            return prefix_matches;
        }
    }

    let term_patterns = compile_term_patterns(&terms);

    let mut results: Vec<ToolSearchResult> = deferred_tools
        .into_iter()
        .filter_map(|tool| {
            let name = tool.name().to_string();
            let description = tool.description().to_string();
            let search_hint = tool.search_hint().map(|s| s.to_string());
            let source = tool.source().map(|s| s.to_string());
            let (name_parts, name_full) = parse_tool_name(&name);
            let description_lower = description.to_lowercase();
            let hint_lower = search_hint.clone().unwrap_or_default().to_lowercase();
            let source_lower = source.clone().unwrap_or_default().to_lowercase();

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
            if source_lower.contains(&query_normalized) {
                score += 25;
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
                if source_lower.contains(term) {
                    score += 6;
                }
                if let Some((_, re)) = term_patterns.iter().find(|(t, _)| t == term) {
                    if re.is_match(&hint_lower) {
                        score += 4;
                    }
                    if re.is_match(&description_lower) {
                        score += 2;
                    }
                    if re.is_match(&source_lower) {
                        score += 3;
                    }
                }
            }

            // Require score ≥ 35 to avoid returning tools that match only via
            // a single CamelCase name part (e.g. "web" matching "WebFetch").
            // A single name-part hit gives 20+10=30; a legitimate match needs
            // at least a hint/description match on top of that.
            if score < 35 {
                return None;
            }

            Some(ToolSearchResult {
                name,
                description,
                search_hint,
                source,
                score,
            })
        })
        .collect();

    results.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
    results.truncate(max_results);
    results
}
