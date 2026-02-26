use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;

use super::get_manager;
use crate::{Tool, ToolContext};

pub struct BrowserSavePDF;

#[async_trait]
impl Tool for BrowserSavePDF {
    fn name(&self) -> &str {
        "BrowserSavePDF"
    }

    fn description(&self) -> &str {
        "Save the current page as a PDF file. The PDF is generated directly via CDP \
         (no print dialog). Returns the file path where the PDF was saved."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "filename": {
                    "type": "string",
                    "description": "Output filename (default: page-title.pdf). Saved to ~/Downloads/lukan/"
                },
                "landscape": {
                    "type": "boolean",
                    "description": "Use landscape orientation (default: false)",
                    "default": false
                },
                "printBackground": {
                    "type": "boolean",
                    "description": "Include background graphics (default: true)",
                    "default": true
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

        let landscape = input
            .get("landscape")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let print_background = input
            .get("printBackground")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // Generate PDF via CDP (no dialog needed)
        let result = match manager
            .send_cdp(
                "Page.printToPDF",
                json!({
                    "landscape": landscape,
                    "printBackground": print_background,
                    "preferCSSPageSize": true,
                }),
            )
            .await
        {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("Failed to generate PDF: {e}"))),
        };

        let data_b64 = match result.get("data").and_then(|d| d.as_str()) {
            Some(d) => d,
            None => return Ok(ToolResult::error("No PDF data returned from Chrome")),
        };

        // Decode base64
        use base64::Engine;
        let pdf_bytes = match base64::engine::general_purpose::STANDARD.decode(data_b64) {
            Ok(bytes) => bytes,
            Err(e) => return Ok(ToolResult::error(format!("Failed to decode PDF data: {e}"))),
        };

        // Determine filename
        let filename = if let Some(name) = input.get("filename").and_then(|v| v.as_str()) {
            let name = name.trim();
            if name.ends_with(".pdf") {
                name.to_string()
            } else {
                format!("{name}.pdf")
            }
        } else {
            // Use page title as filename
            let title = manager
                .send_cdp(
                    "Runtime.evaluate",
                    json!({ "expression": "document.title", "returnByValue": true }),
                )
                .await
                .ok()
                .and_then(|r| {
                    r.get("result")
                        .and_then(|r| r.get("value"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .unwrap_or_default();

            let safe_title: String = title
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect::<String>()
                .trim()
                .to_string();

            if safe_title.is_empty() {
                "page.pdf".to_string()
            } else {
                format!("{safe_title}.pdf")
            }
        };

        // Save to downloads directory
        match manager.save_to_downloads(&filename, &pdf_bytes) {
            Ok(path) => Ok(ToolResult::success(format!(
                "PDF saved to: {}  ({} bytes)",
                path.display(),
                pdf_bytes.len()
            ))),
            Err(e) => Ok(ToolResult::error(format!("Failed to save PDF: {e}"))),
        }
    }
}
