use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use super::get_manager;
use crate::{Tool, ToolContext};

pub struct BrowserTabs;

#[async_trait]
impl Tool for BrowserTabs {
    fn name(&self) -> &str {
        "BrowserTabs"
    }

    fn description(&self) -> &str {
        "List all open browser tabs. Use the tab numbers with BrowserSwitchTab."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let manager = match get_manager() {
            Ok(m) => m,
            Err(e) => return Ok(*e),
        };

        let http_base = match manager.http_base().await {
            Ok(base) => base,
            Err(e) => return Ok(ToolResult::error(format!("Not connected: {e}"))),
        };

        // GET /json to list targets
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;

        let resp = match client.get(format!("{http_base}/json")).send().await {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("Failed to list tabs: {e}"))),
        };

        let targets: Vec<serde_json::Value> = match resp.json().await {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::error(format!("Failed to parse tabs: {e}"))),
        };

        let mut output = String::new();
        let mut tab_num = 0;

        for target in &targets {
            if target.get("type").and_then(|t| t.as_str()) != Some("page") {
                continue;
            }
            tab_num += 1;
            let title = target
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("(untitled)");
            let url = target.get("url").and_then(|u| u.as_str()).unwrap_or("");
            output.push_str(&format!("[{tab_num}] {title}\n    {url}\n"));
        }

        if output.is_empty() {
            output = "(no open tabs)".to_string();
        }

        Ok(ToolResult::success(output.trim_end()))
    }
}
