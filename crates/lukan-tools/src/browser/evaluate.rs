use async_trait::async_trait;
use lukan_core::models::tools::ToolResult;
use regex::Regex;
use serde_json::json;

use super::{browser_tool_metadata, get_manager};
use crate::{Tool, ToolContext};

const MAX_RESULT_LEN: usize = 10_000;

pub struct BrowserEvaluate;

/// Security blocklist — expressions matching any of these are rejected.
fn is_blocked_expression(expr: &str) -> Option<&'static str> {
    let lower = expr.to_lowercase();

    // Build blocklist patterns (compiled once per call — cheap enough)
    let patterns: &[(&str, &str)] = &[
        (r"document\.cookie", "accessing cookies"),
        (r"\.cookie\s*=", "setting cookies"),
        (r"localStorage", "accessing localStorage"),
        (r"sessionStorage", "accessing sessionStorage"),
        (r"indexedDB", "accessing indexedDB"),
        (r"\bfetch\s*\(", "making network requests"),
        (r"XMLHttpRequest", "making network requests"),
        (r"\beval\s*\(", "using eval"),
        (r"Function\s*\(", "using Function constructor"),
        (r"importScripts", "importing scripts"),
        (r"ServiceWorker", "accessing ServiceWorker"),
        (r"navigator\.credentials", "accessing credentials"),
        (r"navigator\.clipboard", "accessing clipboard"),
        (r"navigator\.geolocation", "accessing geolocation"),
        (r"navigator\.mediaDevices", "accessing media devices"),
        (r"Notification\.", "using Notification API"),
        (r"PaymentRequest", "using Payment API"),
        (r"WebSocket\s*\(", "creating WebSocket"),
        (r"EventSource\s*\(", "creating EventSource"),
        (r"SharedWorker", "creating SharedWorker"),
        (r"Worker\s*\(", "creating Worker"),
        (r"crypto\.subtle", "using crypto API"),
        (r"window\.open\s*\(", "opening windows"),
        (r"document\.write", "using document.write"),
        (r"\.innerHTML\s*=", "setting innerHTML"),
        (r"\.outerHTML\s*=", "setting outerHTML"),
        (r"\.insertAdjacentHTML", "using insertAdjacentHTML"),
        (r"document\.domain", "accessing document.domain"),
        (r"postMessage\s*\(", "using postMessage"),
        (r"window\.location\s*=", "setting window.location"),
        (r"location\.href\s*=", "setting location.href"),
        (r"location\.replace", "using location.replace"),
        (r"location\.assign", "using location.assign"),
        (r"history\.(pushState|replaceState)", "modifying history"),
        (
            r#"\.src\s*=\s*['"]?(https?|data|javascript)"#,
            "setting src to URL",
        ),
        (r"javascript:", "using javascript: protocol"),
        (r"chrome\.", "accessing chrome APIs"),
        (r"browser\.", "accessing browser APIs"),
        (r"__proto__", "modifying prototype"),
        (r"prototype\s*\[", "modifying prototype"),
        (r"Object\.defineProperty", "redefining properties"),
        (r"Object\.setPrototypeOf", "setting prototype"),
        (r"Proxy\s*\(", "creating Proxy"),
        (r"Reflect\.", "using Reflect"),
        (r#"\.setAttribute\(\s*['"]on"#, "setting event handlers"),
        ("addEventListener\\(\\s*['\"]", "adding event listeners"),
        (r"setInterval\s*\(", "using setInterval"),
        (r"setTimeout\s*\(", "using setTimeout"),
        (r"requestAnimationFrame", "using requestAnimationFrame"),
        (r"MutationObserver", "using MutationObserver"),
    ];

    for (pattern, reason) in patterns {
        if let Ok(re) = Regex::new(&format!("(?i){pattern}"))
            && re.is_match(&lower)
        {
            return Some(reason);
        }
    }
    None
}

#[async_trait]
impl Tool for BrowserEvaluate {
    fn name(&self) -> &str {
        "BrowserEvaluate"
    }

    fn description(&self) -> &str {
        "Evaluate a JavaScript expression in the browser. Only safe, read-only expressions are allowed (no network, cookies, storage, etc.)."
    }

    browser_tool_metadata!();

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "expression": {
                    "type": "string",
                    "description": "The JavaScript expression to evaluate"
                }
            },
            "required": ["expression"]
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

        let expression = match input.get("expression").and_then(|v| v.as_str()) {
            Some(e) => e,
            None => return Ok(ToolResult::error("Missing required field: expression")),
        };

        // Security check
        if let Some(reason) = is_blocked_expression(expression) {
            return Ok(ToolResult::error(format!(
                "Blocked: {reason}. BrowserEvaluate only allows safe, read-only expressions."
            )));
        }

        match manager
            .send_cdp(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "timeout": 5000,
                }),
            )
            .await
        {
            Ok(result) => {
                // Check for exceptions
                if let Some(exception) = result.get("exceptionDetails") {
                    let msg = exception
                        .get("text")
                        .and_then(|t| t.as_str())
                        .or_else(|| {
                            exception
                                .get("exception")
                                .and_then(|e| e.get("description"))
                                .and_then(|d| d.as_str())
                        })
                        .unwrap_or("Unknown error");
                    return Ok(ToolResult::error(format!("JS error: {msg}")));
                }

                let value = result.get("result").and_then(|r| r.get("value"));

                let mut output = match value {
                    Some(v) => {
                        if let Some(s) = v.as_str() {
                            s.to_string()
                        } else {
                            serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
                        }
                    }
                    None => {
                        // May be a non-serializable value
                        let type_str = result
                            .get("result")
                            .and_then(|r| r.get("type"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("undefined");
                        let desc = result
                            .get("result")
                            .and_then(|r| r.get("description"))
                            .and_then(|d| d.as_str())
                            .unwrap_or("");
                        if desc.is_empty() {
                            type_str.to_string()
                        } else {
                            format!("[{type_str}] {desc}")
                        }
                    }
                };

                if output.len() > MAX_RESULT_LEN {
                    output.truncate(MAX_RESULT_LEN);
                    output.push_str("\n... (result truncated)");
                }

                Ok(ToolResult::success(output))
            }
            Err(e) => Ok(ToolResult::error(format!("Evaluate failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_cookie_access() {
        assert!(is_blocked_expression("document.cookie").is_some());
    }

    #[test]
    fn blocks_fetch() {
        assert!(is_blocked_expression("fetch('https://evil.com')").is_some());
    }

    #[test]
    fn blocks_eval() {
        assert!(is_blocked_expression("eval('alert(1)')").is_some());
    }

    #[test]
    fn blocks_localstorage() {
        assert!(is_blocked_expression("localStorage.getItem('key')").is_some());
    }

    #[test]
    fn allows_safe_expressions() {
        assert!(is_blocked_expression("document.title").is_none());
        assert!(is_blocked_expression("document.querySelectorAll('a').length").is_none());
        assert!(is_blocked_expression("1 + 2").is_none());
        assert!(is_blocked_expression("JSON.stringify({a: 1})").is_none());
    }

    #[test]
    fn blocks_websocket_creation() {
        assert!(is_blocked_expression("new WebSocket('ws://evil.com')").is_some());
    }

    #[test]
    fn blocks_prototype_pollution() {
        assert!(is_blocked_expression("obj.__proto__").is_some());
        assert!(is_blocked_expression("Object.setPrototypeOf(a, b)").is_some());
    }
}
