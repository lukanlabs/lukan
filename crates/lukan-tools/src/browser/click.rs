use std::time::Duration;

use async_trait::async_trait;
use lukan_browser::ax_tree;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use super::{get_manager, wrap_untrusted};
use crate::{Tool, ToolContext};

pub struct BrowserClick;

#[async_trait]
impl Tool for BrowserClick {
    fn name(&self) -> &str {
        "BrowserClick"
    }

    fn description(&self) -> &str {
        "Click on an element identified by its [ref] number from the accessibility snapshot."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "ref": {
                    "type": "integer",
                    "description": "The element reference number from the accessibility snapshot (e.g. 3 for [3])"
                },
                "button": {
                    "type": "string",
                    "description": "Mouse button: left (default), right, middle",
                    "default": "left",
                    "enum": ["left", "right", "middle"]
                }
            },
            "required": ["ref"]
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

        let ref_num = match input.get("ref").and_then(|v| v.as_u64()) {
            Some(n) => n as u32,
            None => return Ok(ToolResult::error("Missing required field: ref")),
        };

        let button = input
            .get("button")
            .and_then(|v| v.as_str())
            .unwrap_or("left");

        // Resolve ref to DOM node
        let entry = match ax_tree::resolve_ref(ref_num) {
            Some(e) => e,
            None => {
                return Ok(ToolResult::error(format!(
                    "Ref [{ref_num}] not found. Run BrowserSnapshot to refresh element references."
                )));
            }
        };

        let backend_id = entry.backend_dom_node_id;

        // Scroll into view
        let _ = manager
            .send_cdp(
                "DOM.scrollIntoViewIfNeeded",
                json!({ "backendNodeId": backend_id }),
            )
            .await;

        // Get box model for click coordinates
        let box_model = manager
            .send_cdp("DOM.getBoxModel", json!({ "backendNodeId": backend_id }))
            .await;

        let (x, y) = match box_model {
            Ok(ref result) => {
                // content quad: [x1,y1, x2,y2, x3,y3, x4,y4]
                let content = result
                    .get("model")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array());

                if let Some(quad) = content {
                    let coords: Vec<f64> = quad.iter().filter_map(|v| v.as_f64()).collect();
                    if coords.len() >= 8 {
                        let cx = (coords[0] + coords[2] + coords[4] + coords[6]) / 4.0;
                        let cy = (coords[1] + coords[3] + coords[5] + coords[7]) / 4.0;
                        (cx, cy)
                    } else {
                        (0.0, 0.0)
                    }
                } else {
                    (0.0, 0.0)
                }
            }
            Err(_) => (0.0, 0.0),
        };

        if x == 0.0 && y == 0.0 {
            return Ok(ToolResult::error(format!(
                "Could not determine position of [{ref_num}] ({} \"{}\")",
                entry.role, entry.name
            )));
        }

        let cdp_button = match button {
            "right" => "right",
            "middle" => "middle",
            _ => "left",
        };
        let click_count = 1;

        // Mouse down + up (fire-and-forget style — click may trigger navigation)
        let _ = manager
            .send_cdp(
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mousePressed",
                    "x": x,
                    "y": y,
                    "button": cdp_button,
                    "clickCount": click_count,
                }),
            )
            .await;

        let _ = manager
            .send_cdp(
                "Input.dispatchMouseEvent",
                json!({
                    "type": "mouseReleased",
                    "x": x,
                    "y": y,
                    "button": cdp_button,
                    "clickCount": click_count,
                }),
            )
            .await;

        // Small settle delay (click may trigger navigation)
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Get updated snapshot
        let snapshot = manager
            .snapshot(false)
            .await
            .unwrap_or_else(|e| format!("(snapshot unavailable: {e})"));

        Ok(ToolResult::success(format!(
            "Clicked [{ref_num}] ({} \"{}\")\n\n{}",
            entry.role,
            entry.name,
            wrap_untrusted(&snapshot)
        )))
    }
}
