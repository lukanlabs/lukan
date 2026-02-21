use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use serde_json::json;
use url::Url;

use crate::{Tool, ToolContext};

const DEFAULT_MAX_LENGTH: usize = 20_000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "WebFetch"
    }

    fn description(&self) -> &str {
        "Fetch content from a URL. HTML is stripped to plain text. Blocks localhost/private IPs."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "max_length": {
                    "type": "integer",
                    "description": "Maximum content length in characters (default: 20000)",
                    "default": 20000
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
        let url_str = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: url"))?;

        let max_length = input
            .get("max_length")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_MAX_LENGTH as u64) as usize;

        // Parse and validate URL
        let url = match Url::parse(url_str) {
            Ok(u) => u,
            Err(e) => return Ok(ToolResult::error(format!("Invalid URL: {e}"))),
        };

        // SSRF check
        if let Some(host) = url.host_str()
            && is_private_host(host)
        {
            return Ok(ToolResult::error(
                "Access to private/local addresses is blocked for security.",
            ));
        }

        // Fetch
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()?;

        let response = match client.get(url.as_str()).send().await {
            Ok(r) => r,
            Err(e) => return Ok(ToolResult::error(format!("Request failed: {e}"))),
        };

        let status = response.status();
        if !status.is_success() {
            return Ok(ToolResult::error(format!(
                "HTTP {}: {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Unknown")
            )));
        }

        let body = match response.text().await {
            Ok(t) => t,
            Err(e) => return Ok(ToolResult::error(format!("Failed to read response: {e}"))),
        };

        // Strip HTML tags for plain text extraction
        let text = strip_html(&body);

        let mut result = text;
        if result.len() > max_length {
            result.truncate(max_length);
            result.push_str("\n... (content truncated)");
        }

        if result.trim().is_empty() {
            Ok(ToolResult::success("(empty response)"))
        } else {
            Ok(ToolResult::success(result))
        }
    }
}

fn is_private_host(host: &str) -> bool {
    if host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "0.0.0.0" {
        return true;
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
            }
            IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
        };
    }

    false
}

/// Basic HTML tag stripping — extracts visible text
fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;
    let mut last_was_space = false;

    let lower = html.to_lowercase();
    let chars: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();

    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            // Check for script/style start/end
            let remaining: String = lower_chars[i..].iter().collect();
            if remaining.starts_with("<script") {
                in_script = true;
            } else if remaining.starts_with("</script") {
                in_script = false;
            } else if remaining.starts_with("<style") {
                in_style = true;
            } else if remaining.starts_with("</style") {
                in_style = false;
            }
            in_tag = true;
            i += 1;
            continue;
        }

        if chars[i] == '>' {
            in_tag = false;
            i += 1;
            continue;
        }

        if in_tag || in_script || in_style {
            i += 1;
            continue;
        }

        // Decode common HTML entities
        if chars[i] == '&' {
            let remaining: String = chars[i..].iter().take(10).collect();
            if remaining.starts_with("&amp;") {
                result.push('&');
                i += 5;
                last_was_space = false;
                continue;
            } else if remaining.starts_with("&lt;") {
                result.push('<');
                i += 4;
                last_was_space = false;
                continue;
            } else if remaining.starts_with("&gt;") {
                result.push('>');
                i += 4;
                last_was_space = false;
                continue;
            } else if remaining.starts_with("&quot;") {
                result.push('"');
                i += 6;
                last_was_space = false;
                continue;
            } else if remaining.starts_with("&nbsp;") {
                result.push(' ');
                i += 6;
                last_was_space = true;
                continue;
            }
        }

        let ch = chars[i];
        if ch.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(ch);
            last_was_space = false;
        }

        i += 1;
    }

    result.trim().to_string()
}
