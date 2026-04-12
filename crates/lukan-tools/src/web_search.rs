use std::time::Duration;

use async_trait::async_trait;
use lukan_core::config::credentials::CredentialsManager;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use crate::{Tool, ToolContext};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_CONTENT: usize = 15_000;

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }

    fn description(&self) -> &str {
        "Search the web for information. Uses Tavily if TAVILY_API_KEY is set, or Brave Search if BRAVE_API_KEY is set."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "count": {
                    "type": "integer",
                    "description": "Number of results to return (default: 5, max: 20)",
                    "default": 5
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let query = match input.get("query").and_then(|v| v.as_str()) {
            Some(q) => q.to_string(),
            None => return Ok(ToolResult::error("Missing required field: query")),
        };
        let count = input
            .get("count")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(20) as usize;

        let creds = CredentialsManager::load().await.unwrap_or_default();

        if let Some(key) = &creds.tavily_api_key
            && !key.is_empty()
        {
            return search_tavily(&query, count, key).await;
        }

        if let Some(key) = &creds.brave_api_key
            && !key.is_empty()
        {
            return search_brave(&query, count, key).await;
        }

        Ok(ToolResult::error(
            "No search API key configured. Set TAVILY_API_KEY or BRAVE_API_KEY in your credentials.",
        ))
    }
}

async fn search_tavily(query: &str, count: usize, api_key: &str) -> anyhow::Result<ToolResult> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    let resp = client
        .post("https://api.tavily.com/search")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&json!({
            "query": query,
            "max_results": count,
            "search_depth": "basic",
            "include_answer": "basic"
        }))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Ok(ToolResult::error(format!("Tavily request failed: {e}"))),
    };

    if !resp.status().is_success() {
        let status = resp.status();
        return Ok(ToolResult::error(format!("Tavily API error: {}", status)));
    }

    let data: serde_json::Value = match resp.json().await {
        Ok(d) => d,
        Err(e) => {
            return Ok(ToolResult::error(format!(
                "Failed to parse Tavily response: {e}"
            )));
        }
    };

    let mut parts: Vec<String> = Vec::new();

    if let Some(answer) = data.get("answer").and_then(|v| v.as_str())
        && !answer.is_empty()
    {
        parts.push(format!("**Answer:** {answer}"));
    }

    if let Some(results) = data.get("results").and_then(|v| v.as_array()) {
        let formatted: Vec<String> = results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let title = r
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(no title)");
                let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let content = r.get("content").and_then(|v| v.as_str()).unwrap_or("");
                format!("{}. {}\n   {}\n   {}", i + 1, title, url, content)
            })
            .collect();
        if !formatted.is_empty() {
            parts.push(formatted.join("\n\n"));
        }
    }

    if parts.is_empty() {
        return Ok(ToolResult::success("No results found."));
    }

    Ok(ToolResult::success(truncate(
        &parts.join("\n\n"),
        MAX_CONTENT,
    )))
}

async fn search_brave(query: &str, count: usize, api_key: &str) -> anyhow::Result<ToolResult> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    let mut url = reqwest::Url::parse("https://api.search.brave.com/res/v1/web/search").unwrap();
    url.query_pairs_mut()
        .append_pair("q", query)
        .append_pair("count", &count.to_string());

    let resp = client
        .get(url)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", api_key)
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Ok(ToolResult::error(format!("Brave request failed: {e}"))),
    };

    if !resp.status().is_success() {
        let status = resp.status();
        return Ok(ToolResult::error(format!(
            "Brave Search API error: {}",
            status
        )));
    }

    let data: serde_json::Value = match resp.json().await {
        Ok(d) => d,
        Err(e) => {
            return Ok(ToolResult::error(format!(
                "Failed to parse Brave response: {e}"
            )));
        }
    };

    let results = data
        .get("web")
        .and_then(|w| w.get("results"))
        .and_then(|r| r.as_array());

    match results {
        None => Ok(ToolResult::success("No results found.")),
        Some(results) if results.is_empty() => Ok(ToolResult::success("No results found.")),
        Some(results) => {
            let formatted: Vec<String> = results
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let title = r
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("(no title)");
                    let url = r.get("url").and_then(|v| v.as_str()).unwrap_or("");
                    let desc = r.get("description").and_then(|v| v.as_str()).unwrap_or("");
                    format!("{}. {}\n   {}\n   {}", i + 1, title, url, desc)
                })
                .collect();
            Ok(ToolResult::success(truncate(
                &formatted.join("\n\n"),
                MAX_CONTENT,
            )))
        }
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_string()
    } else {
        format!("{}\n\n... (truncated)", &text[..max])
    }
}
