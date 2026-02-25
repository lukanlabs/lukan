use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use super::{get_manager, wrap_untrusted};
use crate::{Tool, ToolContext};

pub struct BrowserSwitchTab;

#[async_trait]
impl Tool for BrowserSwitchTab {
    fn name(&self) -> &str {
        "BrowserSwitchTab"
    }

    fn description(&self) -> &str {
        "Switch to a different browser tab by its number (from BrowserTabs)."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "tab": {
                    "type": "integer",
                    "description": "The tab number to switch to (1-indexed, from BrowserTabs output)"
                }
            },
            "required": ["tab"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let manager = match get_manager() {
            Ok(m) => m,
            Err(e) => return Ok(*e),
        };

        let tab_num = match input.get("tab").and_then(|v| v.as_u64()) {
            Some(n) => n as usize,
            None => return Ok(ToolResult::error("Missing required field: tab")),
        };

        if tab_num == 0 {
            return Ok(ToolResult::error("Tab number must be >= 1"));
        }

        let http_base = match manager.http_base().await {
            Ok(base) => base,
            Err(e) => return Ok(ToolResult::error(format!("Not connected: {e}"))),
        };

        // List tabs
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

        // Filter to page targets and find the requested one
        let pages: Vec<&serde_json::Value> = targets
            .iter()
            .filter(|t| t.get("type").and_then(|t| t.as_str()) == Some("page"))
            .collect();

        if tab_num > pages.len() {
            return Ok(ToolResult::error(format!(
                "Tab {tab_num} not found. There are {} open tab(s). Use BrowserTabs to see them.",
                pages.len()
            )));
        }

        let target = pages[tab_num - 1];
        let ws_url = match target.get("webSocketDebuggerUrl").and_then(|u| u.as_str()) {
            Some(u) => u.to_string(),
            None => {
                return Ok(ToolResult::error(
                    "Tab found but no WebSocket URL available",
                ));
            }
        };

        let title = target
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("(untitled)");

        // Switch
        if let Err(e) = manager.switch_to_tab(&ws_url).await {
            return Ok(ToolResult::error(format!("Failed to switch tab: {e}")));
        }

        // Get snapshot
        let snapshot = manager
            .snapshot(false)
            .await
            .unwrap_or_else(|e| format!("(snapshot unavailable: {e})"));

        Ok(ToolResult::success(format!(
            "Switched to tab [{tab_num}] \"{title}\"\n\n{}",
            wrap_untrusted(&snapshot)
        )))
    }
}
