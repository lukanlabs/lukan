use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use super::{browser_tool_metadata, get_manager};
use crate::{Tool, ToolContext};

pub struct BrowserScreenshot;

#[async_trait]
impl Tool for BrowserScreenshot {
    fn name(&self) -> &str {
        "BrowserScreenshot"
    }

    fn description(&self) -> &str {
        "Take a screenshot of the current page. Returns a JPEG image."
    }

    browser_tool_metadata!("capture a screenshot of the current page", "Taking screenshot", read_only = true);

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "quality": {
                    "type": "integer",
                    "description": "JPEG quality (1-100, default: 50)",
                    "default": 50
                },
                "fullPage": {
                    "type": "boolean",
                    "description": "Capture the full scrollable page instead of the viewport",
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

        let quality = input.get("quality").and_then(|v| v.as_u64()).unwrap_or(50);
        let full_page = input
            .get("fullPage")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut params = json!({
            "format": "jpeg",
            "quality": quality,
        });

        // For full-page screenshots, get layout metrics and set the clip
        if full_page
            && let Ok(metrics) = manager.send_cdp("Page.getLayoutMetrics", json!({})).await
            && let Some(content_size) = metrics.get("contentSize")
        {
            params["clip"] = json!({
                "x": 0,
                "y": 0,
                "width": content_size.get("width").and_then(|w| w.as_f64()).unwrap_or(1280.0),
                "height": content_size.get("height").and_then(|h| h.as_f64()).unwrap_or(800.0),
                "scale": 1,
            });
            params["captureBeyondViewport"] = json!(true);
        }

        match manager.send_cdp("Page.captureScreenshot", params).await {
            Ok(result) => {
                let data = result.get("data").and_then(|d| d.as_str()).unwrap_or("");
                let data_url = format!("data:image/jpeg;base64,{data}");
                let mut tool_result = ToolResult::success("Screenshot captured");
                tool_result.image = Some(data_url);
                Ok(tool_result)
            }
            Err(e) => Ok(ToolResult::error(format!("Screenshot failed: {e}"))),
        }
    }
}
