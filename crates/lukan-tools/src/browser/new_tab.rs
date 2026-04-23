use std::time::Duration;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use super::{browser_tool_metadata, get_manager, wrap_untrusted};
use crate::{Tool, ToolContext};

pub struct BrowserNewTab;

#[async_trait]
impl Tool for BrowserNewTab {
    fn name(&self) -> &str {
        "BrowserNewTab"
    }

    fn description(&self) -> &str {
        "Open a new browser tab and navigate to the given URL."
    }

    browser_tool_metadata!(
        "open a new browser tab",
        "Opening browser tab",
        read_only = false
    );

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to open in the new tab"
                }
            },
            "required": ["url"]
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

        let url = match input.get("url").and_then(|v| v.as_str()) {
            Some(u) => u,
            None => return Ok(ToolResult::error("Missing required field: url")),
        };

        // SSRF check
        if let Some(err) = lukan_browser::url_guard::check_url(url) {
            return Ok(ToolResult::error(err));
        }

        let http_base = match manager.http_base().await {
            Ok(base) => base,
            Err(e) => return Ok(ToolResult::error(format!("Not connected: {e}"))),
        };

        // Create new tab via HTTP API
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?;

        let encoded_url = urlencoding::encode(url);
        let resp = match client
            .put(format!("{http_base}/json/new?{encoded_url}"))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("Failed to create tab: {e}"))),
        };

        let target: serde_json::Value = match resp.json().await {
            Ok(t) => t,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to parse tab response: {e}"
                )));
            }
        };

        let ws_url = match target.get("webSocketDebuggerUrl").and_then(|u| u.as_str()) {
            Some(u) => u.to_string(),
            None => {
                return Ok(ToolResult::error(
                    "New tab created but no WebSocket URL returned",
                ));
            }
        };

        // Switch to the new tab
        if let Err(e) = manager.switch_to_tab(&ws_url).await {
            return Ok(ToolResult::error(format!(
                "Tab created but failed to switch: {e}"
            )));
        }

        // Wait for page load
        let _ = manager
            .wait_for_event("Page.loadEventFired", Duration::from_secs(30))
            .await;

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Get snapshot
        let snapshot = manager
            .snapshot(false)
            .await
            .unwrap_or_else(|e| format!("(snapshot unavailable: {e})"));

        Ok(ToolResult::success(format!(
            "Opened new tab: {url}\n\n{}",
            wrap_untrusted(&snapshot)
        )))
    }
}
