use async_trait::async_trait;
use lukan_browser::ax_tree;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use super::{browser_tool_metadata, get_manager, wrap_untrusted};
use crate::{Tool, ToolContext};

pub struct BrowserType;

#[async_trait]
impl Tool for BrowserType {
    fn name(&self) -> &str {
        "BrowserType"
    }

    fn description(&self) -> &str {
        "Type text into an input field identified by its [ref] number from the accessibility snapshot."
    }

    browser_tool_metadata!(
        "type text into a browser element",
        "Typing in browser",
        read_only = false
    );

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "ref": {
                    "type": "integer",
                    "description": "The element reference number from the accessibility snapshot"
                },
                "text": {
                    "type": "string",
                    "description": "The text to type"
                },
                "clear": {
                    "type": "boolean",
                    "description": "Clear the field before typing (default: true)",
                    "default": true
                },
                "submit": {
                    "type": "boolean",
                    "description": "Press Enter after typing (default: false)",
                    "default": false
                }
            },
            "required": ["ref", "text"]
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

        let text = match input.get("text").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return Ok(ToolResult::error("Missing required field: text")),
        };

        let clear = input.get("clear").and_then(|v| v.as_bool()).unwrap_or(true);
        let submit = input
            .get("submit")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Resolve ref
        let entry = match ax_tree::resolve_ref(ref_num) {
            Some(e) => e,
            None => {
                return Ok(ToolResult::error(format!(
                    "Ref [{ref_num}] not found. Run BrowserSnapshot to refresh element references."
                )));
            }
        };

        let backend_id = entry.backend_dom_node_id;

        // Resolve backend node to a remote object for JS calls
        let resolve_result = manager
            .send_cdp("DOM.resolveNode", json!({ "backendNodeId": backend_id }))
            .await;

        let object_id = match resolve_result {
            Ok(ref result) => result
                .get("object")
                .and_then(|o| o.get("objectId"))
                .and_then(|id| id.as_str())
                .map(|s| s.to_string()),
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Failed to resolve element [{ref_num}]: {e}"
                )));
            }
        };

        let object_id = match object_id {
            Some(id) => id,
            None => {
                return Ok(ToolResult::error(format!(
                    "Could not get objectId for [{ref_num}]"
                )));
            }
        };

        // Focus the element
        let _ = manager
            .send_cdp(
                "Runtime.callFunctionOn",
                json!({
                    "objectId": object_id,
                    "functionDeclaration": "function() { this.focus(); }",
                    "arguments": [],
                }),
            )
            .await;

        // Clear the field if requested
        if clear {
            let _ = manager
                .send_cdp(
                    "Runtime.callFunctionOn",
                    json!({
                        "objectId": object_id,
                        "functionDeclaration": r#"function() {
                            // Use native setter to trigger React/Vue change events
                            var nativeSetter = Object.getOwnPropertyDescriptor(
                                window.HTMLInputElement.prototype, 'value'
                            ) || Object.getOwnPropertyDescriptor(
                                window.HTMLTextAreaElement.prototype, 'value'
                            );
                            if (nativeSetter && nativeSetter.set) {
                                nativeSetter.set.call(this, '');
                            } else {
                                this.value = '';
                            }
                            this.dispatchEvent(new Event('input', { bubbles: true }));
                            this.dispatchEvent(new Event('change', { bubbles: true }));
                        }"#,
                        "arguments": [],
                    }),
                )
                .await;
        }

        // Type the text using Input.insertText
        if let Err(e) = manager
            .send_cdp("Input.insertText", json!({ "text": text }))
            .await
        {
            return Ok(ToolResult::error(format!("Failed to type text: {e}")));
        }

        // Submit with Enter if requested
        if submit {
            for event_type in &["keyDown", "keyUp"] {
                let _ = manager
                    .send_cdp(
                        "Input.dispatchKeyEvent",
                        json!({
                            "type": event_type,
                            "key": "Enter",
                            "code": "Enter",
                            "windowsVirtualKeyCode": 13,
                            "nativeVirtualKeyCode": 13,
                        }),
                    )
                    .await;
            }

            // Wait for potential navigation
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        // Get updated snapshot
        let snapshot = manager
            .snapshot(false)
            .await
            .unwrap_or_else(|e| format!("(snapshot unavailable: {e})"));

        let submit_note = if submit { " and submitted" } else { "" };
        Ok(ToolResult::success(format!(
            "Typed \"{text}\" into [{ref_num}] ({} \"{}\"){submit_note}\n\n{}",
            entry.role,
            entry.name,
            wrap_untrusted(&snapshot)
        )))
    }
}
