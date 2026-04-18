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

pub fn search_deferred_tools(
    registry: &ToolRegistry,
    query: &str,
    max_results: usize,
) -> Vec<ToolSearchResult> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    if terms.is_empty() {
        return Vec::new();
    }

    let mut results: Vec<ToolSearchResult> = registry
        .tools
        .values()
        .filter(|tool| tool.is_deferred())
        .filter_map(|tool| {
            let name = tool.name().to_string();
            let description = tool.description().to_string();
            let search_hint = tool.search_hint().map(|s| s.to_string());

            let haystacks = [
                name.to_lowercase(),
                description.to_lowercase(),
                search_hint.clone().unwrap_or_default().to_lowercase(),
            ];

            let mut score = 0usize;
            for term in &terms {
                if haystacks[0].contains(term) {
                    score += 3;
                }
                if haystacks[1].contains(term) {
                    score += 2;
                }
                if haystacks[2].contains(term) {
                    score += 2;
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
