use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use super::{get_manager, wrap_untrusted};
use crate::{Tool, ToolContext};

pub struct BrowserSnapshot;

#[async_trait]
impl Tool for BrowserSnapshot {
    fn name(&self) -> &str {
        "BrowserSnapshot"
    }

    fn description(&self) -> &str {
        "Return the current page's accessibility snapshot. Interactive elements are numbered [1], [2], ... for use with BrowserClick and BrowserType."
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

        match manager.snapshot().await {
            Ok(snapshot) => Ok(ToolResult::success(wrap_untrusted(&snapshot))),
            Err(e) => Ok(ToolResult::error(format!("Failed to get snapshot: {e}"))),
        }
    }
}
