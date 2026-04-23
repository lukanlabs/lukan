use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use super::{browser_tool_metadata, get_manager, wrap_untrusted};
use crate::{Tool, ToolContext};

pub struct BrowserSnapshot;

#[async_trait]
impl Tool for BrowserSnapshot {
    fn name(&self) -> &str {
        "BrowserSnapshot"
    }

    fn description(&self) -> &str {
        "Return the current page's accessibility snapshot. Interactive elements are numbered [1], [2], ... for use with BrowserClick and BrowserType. Use compact mode to save tokens when you only need interactive elements."
    }

    browser_tool_metadata!(
        "capture an accessibility snapshot of the current page",
        "Snapshotting page",
        read_only = true
    );

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "compact": {
                    "type": "boolean",
                    "description": "If true, return only interactive elements (buttons, links, inputs, etc.) without static text or structural markers. Reduces output by ~50-70%. Default: false",
                    "default": false
                }
            },
            "required": []
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

        let compact = input
            .get("compact")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        match manager.snapshot(compact).await {
            Ok(snapshot) => Ok(ToolResult::success(wrap_untrusted(&snapshot))),
            Err(e) => Ok(ToolResult::error(format!("Failed to get snapshot: {e}"))),
        }
    }
}
