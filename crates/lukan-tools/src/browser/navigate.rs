use std::time::Duration;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use super::{browser_tool_metadata, get_manager, wrap_untrusted};
use crate::{Tool, ToolContext};

pub struct BrowserNavigate;

#[async_trait]
impl Tool for BrowserNavigate {
    fn name(&self) -> &str {
        "BrowserNavigate"
    }

    fn description(&self) -> &str {
        "Navigate the browser to a URL. Returns an accessibility snapshot of the page."
    }

    browser_tool_metadata!(
        "navigate and inspect pages in the browser",
        "Navigating browser",
        read_only = false
    );

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to navigate to"
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

        // Navigate
        if let Err(e) = manager
            .send_cdp("Page.navigate", json!({ "url": url }))
            .await
        {
            return Ok(ToolResult::error(format!("Navigation failed: {e}")));
        }

        // Wait for page load
        let _ = manager
            .wait_for_event("Page.loadEventFired", Duration::from_secs(30))
            .await;

        // Small settle delay
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Get snapshot
        let snapshot = match manager.snapshot(false).await {
            Ok(s) => s,
            Err(e) => return Ok(ToolResult::error(format!("Failed to get snapshot: {e}"))),
        };

        // Quick screenshot
        let image = manager.quick_screenshot().await.ok();

        let mut result = ToolResult::success(format!(
            "Navigated to {url}\n\n{}",
            wrap_untrusted(&snapshot)
        ));
        result.image = image;
        Ok(result)
    }
}
