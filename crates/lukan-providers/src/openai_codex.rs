//! OpenAI Codex provider — uses the Responses API with OAuth bearer auth.

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use lukan_core::config::{Credentials, CredentialsManager};
use lukan_core::models::events::{StopReason, StreamEvent};
use lukan_core::models::messages::{ContentBlock, ImageSource, Message, MessageContent, Role};
use regex::Regex;
use reqwest::Client;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, warn};

use crate::codex_auth::{self, CodexTokens};
use crate::contracts::{Provider, StreamParams, SystemPrompt};

// ── Constants ─────────────────────────────────────────────────────────────

const RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";

const CODEX_INSTRUCTIONS: &str = "\
You are an AI coding agent. You MUST use function calls to perform actions.

## Rules
- When asked to do something, call the appropriate tool immediately. Do not describe what you plan to do.
- Keep going until the task is fully resolved before ending your turn.
- If you need information, call a tool to get it. Do not guess.
- After tool results, proceed to the next step or give a brief summary.
- Be concise. Answer in the user's language.";

// ── Provider ──────────────────────────────────────────────────────────────

pub struct OpenAICodexProvider {
    client: Client,
    model: String,
    max_tokens: u32,
    tokens: Arc<Mutex<CodexTokens>>,
    credentials: Credentials,
    reasoning_effort: std::sync::Mutex<String>,
}

impl OpenAICodexProvider {
    pub fn new(model: String, max_tokens: u32, credentials: Credentials) -> Result<Self> {
        let access_token = credentials
            .codex_access_token
            .as_deref()
            .context("No Codex access token found. Run 'lukan codex-auth' to authenticate.")?
            .to_string();

        let refresh_token = credentials.codex_refresh_token.clone().unwrap_or_default();

        let expires_at = credentials.codex_token_expiry.unwrap_or(0);

        let tokens = CodexTokens {
            access_token,
            refresh_token,
            expires_at,
        };

        Ok(Self {
            client: Client::new(),
            model,
            max_tokens,
            tokens: Arc::new(Mutex::new(tokens)),
            credentials,
            reasoning_effort: std::sync::Mutex::new("medium".to_string()),
        })
    }

    /// Get a valid access token, refreshing if needed.
    async fn get_access_token(&self) -> Result<String> {
        let mut tokens = self.tokens.lock().await;

        if codex_auth::needs_refresh(&tokens) {
            if tokens.refresh_token.is_empty() {
                bail!(
                    "Codex token expired and no refresh token available. \
                     Run 'lukan codex-auth' to re-authenticate."
                );
            }

            debug!("Refreshing Codex access token...");
            let new_tokens =
                codex_auth::refresh_tokens(&self.client, &tokens.refresh_token).await?;

            // Persist refreshed tokens
            let mut creds = self.credentials.clone();
            creds.codex_access_token = Some(new_tokens.access_token.clone());
            creds.codex_refresh_token = Some(new_tokens.refresh_token.clone());
            creds.codex_token_expiry = Some(new_tokens.expires_at);
            if let Err(e) = CredentialsManager::save(&creds).await {
                warn!("Failed to persist refreshed tokens: {e}");
            }

            *tokens = new_tokens;
        }

        Ok(tokens.access_token.clone())
    }

    /// Check if the model is a reasoning model.
    fn is_reasoning_model(&self) -> bool {
        self.model.contains("codex") || self.model.starts_with('o')
    }
}

#[async_trait]
impl Provider for OpenAICodexProvider {
    fn name(&self) -> &str {
        "openai-codex"
    }

    fn supports_images(&self) -> bool {
        true
    }

    fn set_reasoning_effort(&self, effort: &str) {
        if let Ok(mut e) = self.reasoning_effort.lock() {
            *e = effort.to_string();
        }
    }

    fn reasoning_effort(&self) -> Option<&'static str> {
        // Return a static str based on current value
        let effort = self.reasoning_effort.lock().ok()?;
        match effort.as_str() {
            "low" => Some("low"),
            "high" => Some("high"),
            "extra_high" => Some("extra_high"),
            _ => Some("medium"),
        }
    }

    async fn stream(&self, params: StreamParams, tx: mpsc::Sender<StreamEvent>) -> Result<()> {
        let access_token = self.get_access_token().await?;
        let account_id = codex_auth::extract_account_id(&access_token);

        // Build instructions
        let system_text = match &params.system_prompt {
            SystemPrompt::Text(t) => t.clone(),
            SystemPrompt::Structured { cached, dynamic } => {
                let mut parts = cached.clone();
                parts.push(dynamic.clone());
                parts.join("\n")
            }
        };
        let instructions = if system_text.is_empty() {
            CODEX_INSTRUCTIONS.to_string()
        } else {
            format!("{system_text}\n{CODEX_INSTRUCTIONS}")
        };

        // Convert messages to Responses API input
        let input = convert_messages(&params.messages);

        // Convert tools
        let tools: Vec<Value> = params
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema,
                    "strict": false,
                })
            })
            .collect();

        // Build request body
        let mut body = json!({
            "model": self.model,
            "instructions": instructions,
            "input": input,
            "stream": true,
            "store": false,
            "tool_choice": "auto",
            "parallel_tool_calls": false,
        });

        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }

        if self.is_reasoning_model() {
            let effort = self.reasoning_effort.lock().unwrap().clone();
            body["reasoning"] = json!({
                "effort": effort,
                "summary": "auto",
            });
        }

        // Send request
        let mut req = self
            .client
            .post(RESPONSES_URL)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {access_token}"))
            .header("Accept", "text/event-stream");

        if let Some(ref acct_id) = account_id {
            req = req.header("ChatGPT-Account-Id", acct_id);
        }

        let resp = req
            .body(body.to_string())
            .send()
            .await
            .context("Codex API request failed")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
            {
                bail!(
                    "Codex authentication failed ({status}). Run 'lukan codex-auth' to re-authenticate.\n{body}"
                );
            }
            bail!("Codex API error ({status}): {body}");
        }

        tx.send(StreamEvent::MessageStart).await.ok();

        // Parse SSE stream with event: + data: format
        parse_codex_sse(resp, &tx).await?;

        Ok(())
    }
}

// ── SSE Parsing ───────────────────────────────────────────────────────────

async fn parse_codex_sse(resp: reqwest::Response, tx: &mpsc::Sender<StreamEvent>) -> Result<()> {
    use futures::StreamExt;

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut current_event = String::new();

    // Track tool calls: item_id -> accumulated arguments
    let mut arg_buffers: HashMap<String, String> = HashMap::new();
    // Track which item IDs map to which call_id + name
    let mut tool_calls: HashMap<String, (String, String)> = HashMap::new();
    let mut has_tool_calls = false;
    let mut stop_reason = StopReason::EndTurn;
    // Track the last item_id seen for fallback
    let mut last_item_id = String::new();

    // Phantom tool call detection: some models emit tool calls as text
    // instead of proper function_call events. We buffer text and detect these.
    let mut text_buffer = String::new();
    let mut phantom_buffer = String::new();
    let mut phantom_suppressed = false;

    let chunk_timeout = std::time::Duration::from_secs(60);
    while let Some(chunk) = tokio::time::timeout(chunk_timeout, stream.next())
        .await
        .context("Provider stream timed out (no data for 60s)")?
    {
        let chunk = chunk.context("Stream read error")?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(newline_pos) = buffer.find('\n') {
            let line = buffer[..newline_pos].trim_end_matches('\r').to_string();
            buffer = buffer[newline_pos + 1..].to_string();

            if line.is_empty() {
                // Event delimiter — reset event type
                current_event.clear();
                continue;
            }

            if let Some(event_type) = line.strip_prefix("event: ") {
                current_event = event_type.to_string();
                continue;
            }

            let Some(data_str) = line.strip_prefix("data: ") else {
                continue;
            };

            if data_str == "[DONE]" {
                break;
            }

            let data: Value = match serde_json::from_str(data_str) {
                Ok(v) => v,
                Err(e) => {
                    debug!("Skipping malformed SSE data: {e}");
                    continue;
                }
            };

            // Flush text buffer on non-text events
            if current_event != "response.output_text.delta" && !text_buffer.is_empty() {
                tx.send(StreamEvent::TextDelta {
                    text: std::mem::take(&mut text_buffer),
                })
                .await
                .ok();
            }

            // Try to parse phantom buffer as a real tool call
            if current_event != "response.output_text.delta" && !phantom_buffer.is_empty() {
                if let Some((name, input)) = extract_phantom_tool_call(&phantom_buffer) {
                    debug!("Recovered phantom tool call: {name}");
                    let call_id = format!("phantom_{}", uuid::Uuid::new_v4());
                    has_tool_calls = true;
                    tx.send(StreamEvent::ToolUseStart {
                        id: call_id.clone(),
                        name: name.clone(),
                    })
                    .await
                    .ok();
                    tx.send(StreamEvent::ToolUseEnd {
                        id: call_id,
                        name,
                        input,
                    })
                    .await
                    .ok();
                } else {
                    debug!(
                        "Discarded phantom text ({} chars): {}",
                        phantom_buffer.len(),
                        &phantom_buffer[..phantom_buffer.len().min(300)]
                    );
                }
                phantom_buffer.clear();
                phantom_suppressed = false;
            }

            // Log every SSE event for debugging
            debug!(
                "SSE event: {} | data_keys: {:?}",
                current_event,
                data.as_object().map(|o| o.keys().collect::<Vec<_>>())
            );

            match current_event.as_str() {
                "response.output_item.added" => {
                    if data["item"]["type"].as_str() == Some("function_call") {
                        let item = &data["item"];
                        let item_id = item["id"].as_str().unwrap_or("").to_string();
                        let call_id = item["call_id"]
                            .as_str()
                            .or_else(|| item["id"].as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = item["name"].as_str().unwrap_or("").to_string();

                        last_item_id.clone_from(&item_id);
                        tool_calls.insert(item_id.clone(), (call_id.clone(), name.clone()));
                        arg_buffers.insert(item_id, String::new());
                        has_tool_calls = true;

                        tx.send(StreamEvent::ToolUseStart { id: call_id, name })
                            .await
                            .ok();
                    }
                }

                "response.output_text.delta" => {
                    if let Some(delta) = data["delta"].as_str() {
                        if phantom_suppressed {
                            // Already in phantom mode — accumulate for possible tool extraction
                            phantom_buffer.push_str(delta);
                        } else {
                            // Append to text buffer and check for phantom patterns
                            text_buffer.push_str(delta);

                            // Check for phantom tool call patterns in accumulated text
                            let phantom_triggers = ["to=functions.", "+#+#+#+#", "assistant to="];
                            let mut trigger_pos = None;
                            for trigger in &phantom_triggers {
                                if let Some(pos) = text_buffer.find(trigger) {
                                    trigger_pos = Some(pos);
                                    break;
                                }
                            }

                            if let Some(pos) = trigger_pos {
                                // Emit clean text before the phantom pattern
                                let clean = &text_buffer[..pos];
                                let trimmed = clean.trim_end();
                                if !trimmed.is_empty() {
                                    tx.send(StreamEvent::TextDelta {
                                        text: trimmed.to_string(),
                                    })
                                    .await
                                    .ok();
                                }
                                // Move remainder to phantom buffer
                                phantom_buffer = text_buffer[pos..].to_string();
                                text_buffer.clear();
                                phantom_suppressed = true;
                                debug!("Phantom tool call detected, suppressing text output");
                            } else {
                                // No phantom detected — emit all safe text
                                // Keep last 30 chars as lookback in case a trigger spans deltas
                                let lookback_chars = 30;
                                let char_count = text_buffer.chars().count();
                                if char_count > lookback_chars {
                                    let emit_chars = char_count - lookback_chars;
                                    let safe_len = text_buffer
                                        .char_indices()
                                        .nth(emit_chars)
                                        .map(|(i, _)| i)
                                        .unwrap_or(0);
                                    if safe_len > 0 {
                                        let emit = &text_buffer[..safe_len];
                                        tx.send(StreamEvent::TextDelta {
                                            text: emit.to_string(),
                                        })
                                        .await
                                        .ok();
                                        text_buffer = text_buffer[safe_len..].to_string();
                                    }
                                }
                            }
                        }
                    }
                }

                "response.reasoning.delta" | "response.reasoning_summary_text.delta" => {
                    if let Some(delta) = data["delta"].as_str() {
                        tx.send(StreamEvent::ThinkingDelta {
                            text: delta.to_string(),
                        })
                        .await
                        .ok();
                    }
                }

                "response.function_call_arguments.delta" => {
                    let item_id = data["item_id"]
                        .as_str()
                        .unwrap_or(&last_item_id)
                        .to_string();

                    if let Some(delta) = data["delta"].as_str() {
                        if let Some(buf) = arg_buffers.get_mut(&item_id) {
                            buf.push_str(delta);
                        }
                        tx.send(StreamEvent::ToolUseDelta {
                            input: delta.to_string(),
                        })
                        .await
                        .ok();
                    }
                }

                "response.output_item.done" => {
                    if data["item"]["type"].as_str() == Some("function_call") {
                        let item = &data["item"];
                        let item_id = item["id"].as_str().unwrap_or("").to_string();

                        let (call_id, name) = tool_calls
                            .remove(&item_id)
                            .unwrap_or_else(|| (item_id.clone(), String::new()));

                        // Get arguments: prefer item.arguments, fall back to accumulated buffer
                        let raw_args = item["arguments"]
                            .as_str()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| arg_buffers.remove(&item_id).unwrap_or_default());

                        let input = parse_tool_input(&raw_args);
                        arg_buffers.remove(&item_id);

                        tx.send(StreamEvent::ToolUseEnd {
                            id: call_id,
                            name,
                            input,
                        })
                        .await
                        .ok();
                    }
                }

                "response.completed" => {
                    debug!(
                        "Stream completed. has_tool_calls={has_tool_calls}, phantom_suppressed={phantom_suppressed}, phantom_buffer_len={}",
                        phantom_buffer.len()
                    );
                    let response = &data["response"];

                    // Usage
                    if let Some(usage) = response.get("usage") {
                        let input_tokens = usage["input_tokens"].as_u64().unwrap_or(0);
                        let output_tokens = usage["output_tokens"].as_u64().unwrap_or(0);
                        // OpenAI returns cached tokens inside input_tokens_details
                        let cached = usage
                            .get("input_tokens_details")
                            .and_then(|d| d.get("cached_tokens"))
                            .and_then(|v| v.as_u64())
                            .filter(|&v| v > 0);
                        tx.send(StreamEvent::Usage {
                            input_tokens,
                            output_tokens,
                            cache_creation_tokens: None,
                            cache_read_tokens: cached,
                        })
                        .await
                        .ok();
                    }

                    // Check for incomplete
                    if response["status"].as_str() == Some("incomplete")
                        && let Some(details) = response.get("incomplete_details")
                        && details["reason"].as_str() == Some("max_output_tokens")
                    {
                        stop_reason = StopReason::MaxTokens;
                    }
                }

                "response.failed" => {
                    debug!(
                        "Stream FAILED: {}",
                        serde_json::to_string_pretty(&data).unwrap_or_default()
                    );
                    let error_msg = data["response"]["error"]["message"]
                        .as_str()
                        .or_else(|| data["error"]["message"].as_str())
                        .or_else(|| data["response"]["error"].as_str())
                        .unwrap_or("Unknown Codex error");

                    tx.send(StreamEvent::Error {
                        error: error_msg.to_string(),
                    })
                    .await
                    .ok();

                    stop_reason = StopReason::Error;
                }

                "response.incomplete" => {
                    if let Some(details) = data["response"].get("incomplete_details")
                        && details["reason"].as_str() == Some("max_output_tokens")
                    {
                        stop_reason = StopReason::MaxTokens;
                    }
                }

                // Ignored events
                _ => {}
            }
        }
    }

    // Flush remaining text buffer
    if !text_buffer.is_empty() {
        tx.send(StreamEvent::TextDelta {
            text: std::mem::take(&mut text_buffer),
        })
        .await
        .ok();
    }

    // Try to recover phantom tool call from buffer
    if !phantom_buffer.is_empty() {
        if let Some((name, input)) = extract_phantom_tool_call(&phantom_buffer) {
            debug!("Recovered phantom tool call at end: {name}");
            let call_id = format!("phantom_{}", uuid::Uuid::new_v4());
            has_tool_calls = true;
            tx.send(StreamEvent::ToolUseStart {
                id: call_id.clone(),
                name: name.clone(),
            })
            .await
            .ok();
            tx.send(StreamEvent::ToolUseEnd {
                id: call_id,
                name,
                input,
            })
            .await
            .ok();
        } else {
            debug!(
                "Discarded phantom text at end: {}",
                &phantom_buffer[..phantom_buffer.len().min(100)]
            );
        }
    }

    // Determine final stop reason
    if has_tool_calls && stop_reason != StopReason::Error {
        stop_reason = StopReason::ToolUse;
    }

    debug!(
        "Codex stream done: stop_reason={stop_reason:?}, has_tool_calls={has_tool_calls}, text_buffer_len={}, phantom_buffer_len={}",
        text_buffer.len(),
        phantom_buffer.len()
    );

    tx.send(StreamEvent::MessageEnd { stop_reason }).await.ok();

    Ok(())
}

/// Extract a tool name and JSON input from phantom tool call text.
///
/// Phantom text looks like:
///   "assistant to=functions.ReadFile <garbled> json {"file_path": "..."}"
///   "+#+#+#+assistant to=functions.Glob <garbled> json {"pattern": "..."}"
fn extract_phantom_tool_call(text: &str) -> Option<(String, Value)> {
    // Extract tool name: "to=functions.TOOLNAME"
    let func_marker = "to=functions.";
    let func_pos = text.find(func_marker)?;
    let after_func = &text[func_pos + func_marker.len()..];

    // Tool name ends at first non-alphanumeric/underscore
    let name_end = after_func
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(after_func.len());
    let tool_name = &after_func[..name_end];
    if tool_name.is_empty() {
        return None;
    }

    // Extract JSON: find first '{' and match braces
    let Some(json_start) = text.find('{') else {
        // No JSON found — args are garbled. Emit tool call with empty input
        // so the tool fails gracefully and the agent loop continues.
        debug!("Phantom tool call '{tool_name}': no JSON args found, using empty input");
        return Some((tool_name.to_string(), Value::Object(Default::default())));
    };
    let json_text = &text[json_start..];

    // Find matching closing brace
    let mut depth = 0;
    let mut end = 0;
    for (i, c) in json_text.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }

    if end == 0 {
        // Unclosed JSON — also use empty input
        debug!("Phantom tool call '{tool_name}': unclosed JSON, using empty input");
        return Some((tool_name.to_string(), Value::Object(Default::default())));
    }

    let input = parse_tool_input(&json_text[..end]);
    Some((tool_name.to_string(), input))
}

// ── Message Conversion ────────────────────────────────────────────────────

fn convert_messages(messages: &[Message]) -> Vec<Value> {
    let mut items: Vec<Value> = Vec::new();

    for msg in messages {
        match msg.role {
            Role::User => convert_user_message(msg, &mut items),
            Role::Assistant => convert_assistant_message(msg, &mut items),
            Role::Tool => convert_tool_message(msg, &mut items),
        }
    }

    items
}

fn convert_user_message(msg: &Message, items: &mut Vec<Value>) {
    match &msg.content {
        MessageContent::Text(text) => {
            items.push(json!({
                "role": "user",
                "content": text,
            }));
        }
        MessageContent::Blocks(blocks) => {
            let mut content_parts: Vec<Value> = Vec::new();

            for block in blocks {
                match block {
                    ContentBlock::Text { text } => {
                        content_parts.push(json!({
                            "type": "input_text",
                            "text": text,
                        }));
                    }
                    ContentBlock::Image {
                        source,
                        data,
                        media_type,
                    } => {
                        let url = match source {
                            ImageSource::Url => data.clone(),
                            ImageSource::Base64 => {
                                let mt = media_type.as_deref().unwrap_or("image/jpeg");
                                format!("data:{mt};base64,{data}")
                            }
                        };
                        content_parts.push(json!({
                            "type": "input_image",
                            "image_url": url,
                        }));
                    }
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                        ..
                    } => {
                        // Flush accumulated content parts as a user message
                        if !content_parts.is_empty() {
                            items.push(json!({
                                "role": "user",
                                "content": content_parts,
                            }));
                            content_parts = Vec::new();
                        }

                        let output = if is_error.unwrap_or(false) {
                            format!("Error: {content}")
                        } else {
                            content.clone()
                        };

                        items.push(json!({
                            "type": "function_call_output",
                            "call_id": tool_use_id,
                            "output": output,
                        }));
                    }
                    _ => {}
                }
            }

            if !content_parts.is_empty() {
                items.push(json!({
                    "role": "user",
                    "content": content_parts,
                }));
            }
        }
    }
}

fn convert_assistant_message(msg: &Message, items: &mut Vec<Value>) {
    match &msg.content {
        MessageContent::Text(text) => {
            items.push(json!({
                "role": "assistant",
                "content": [{ "type": "output_text", "text": text }],
            }));
        }
        MessageContent::Blocks(blocks) => {
            let mut text_buffer = String::new();

            for block in blocks {
                match block {
                    ContentBlock::Text { text } => {
                        text_buffer.push_str(text);
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        // Flush text buffer
                        if !text_buffer.is_empty() {
                            items.push(json!({
                                "role": "assistant",
                                "content": [{ "type": "output_text", "text": text_buffer }],
                            }));
                            text_buffer.clear();
                        }

                        let arguments = if input.is_string() {
                            input.as_str().unwrap().to_string()
                        } else {
                            serde_json::to_string(input).unwrap_or_default()
                        };

                        items.push(json!({
                            "type": "function_call",
                            "call_id": id,
                            "name": name,
                            "arguments": arguments,
                        }));
                    }
                    ContentBlock::Thinking { .. } => {
                        // Skip thinking blocks
                    }
                    _ => {}
                }
            }

            if !text_buffer.is_empty() {
                items.push(json!({
                    "role": "assistant",
                    "content": [{ "type": "output_text", "text": text_buffer }],
                }));
            }
        }
    }
}

fn convert_tool_message(msg: &Message, items: &mut Vec<Value>) {
    let call_id = msg.tool_call_id.as_deref().unwrap_or("");
    let output = msg.content.to_text();

    items.push(json!({
        "type": "function_call_output",
        "call_id": call_id,
        "output": output,
    }));
}

// ── Tool Input Parsing ────────────────────────────────────────────────────

fn parse_tool_input(raw: &str) -> Value {
    // Try direct parse
    if let Ok(parsed) = serde_json::from_str::<Value>(raw)
        && parsed.is_object()
    {
        return parsed;
    }

    // Try normalization
    if let Some(normalized) = normalize_tool_input(raw)
        && let Ok(parsed) = serde_json::from_str::<Value>(&normalized)
        && parsed.is_object()
    {
        return parsed;
    }

    // Default to empty object
    json!({})
}

fn normalize_tool_input(raw: &str) -> Option<String> {
    let mut s = raw.trim().to_string();
    if s.is_empty() {
        return Some("{}".to_string());
    }

    // Strip markdown fences
    let fence_re = Regex::new(r"(?s)^```(?:json)?\s*(.*?)\s*```$").ok()?;
    if let Some(caps) = fence_re.captures(&s) {
        s = caps[1].trim().to_string();
    }

    // Extract function-style: FunctionName({...})
    let fn_re = Regex::new(r"(?s)^[A-Za-z_][A-Za-z0-9_]*\((.*)\)$").ok()?;
    if let Some(caps) = fn_re.captures(&s) {
        s = caps[1].trim().to_string();
    }

    // Wrap key:value pairs without braces
    if !s.starts_with('{') && !s.starts_with('[') && s.contains(':') {
        s = format!("{{{s}}}");
    }

    // Normalize curly quotes
    s = s.replace(['\u{201C}', '\u{201D}', '\u{2018}', '\u{2019}'], "\"");

    // Quote unquoted keys: {key: -> {"key":
    let key_re = Regex::new(r#"([{,]\s*)([A-Za-z_][A-Za-z0-9_-]*)(\s*:)"#).ok()?;
    s = key_re.replace_all(&s, r#"$1"$2"$3"#).to_string();

    // Remove trailing commas
    let trailing_re = Regex::new(r",\s*([}\]])").ok()?;
    s = trailing_re.replace_all(&s, "$1").to_string();

    // Final validation
    if serde_json::from_str::<Value>(&s).is_ok() {
        Some(s)
    } else {
        None
    }
}

// ── Model List ────────────────────────────────────────────────────────────

/// Available Codex models
pub fn codex_models() -> Vec<String> {
    vec![
        "gpt-5.4".to_string(),
        "gpt-5.3-codex".to_string(),
        "gpt-5.3-codex-spark".to_string(),
        "gpt-5.2-codex".to_string(),
        "gpt-5.1-codex-max".to_string(),
        "gpt-5.1-codex".to_string(),
        "gpt-5.1-codex-mini".to_string(),
        "gpt-5.2".to_string(),
        "gpt-5.1".to_string(),
        "gpt-5-codex".to_string(),
        "gpt-5-codex-mini".to_string(),
        "gpt-5".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_tool_input_valid_json() {
        let input = r#"{"path": "/tmp/test.txt", "content": "hello"}"#;
        let result = parse_tool_input(input);
        assert_eq!(result["path"], "/tmp/test.txt");
        assert_eq!(result["content"], "hello");
    }

    #[test]
    fn test_parse_tool_input_empty() {
        let result = parse_tool_input("");
        assert_eq!(result, json!({}));
    }

    #[test]
    fn test_parse_tool_input_markdown_fenced() {
        let input = "```json\n{\"path\": \"/tmp/test.txt\"}\n```";
        let result = parse_tool_input(input);
        assert_eq!(result["path"], "/tmp/test.txt");
    }

    #[test]
    fn test_parse_tool_input_unquoted_keys() {
        let input = "{path: \"/tmp/test.txt\"}";
        let result = parse_tool_input(input);
        assert_eq!(result["path"], "/tmp/test.txt");
    }

    #[test]
    fn test_parse_tool_input_trailing_comma() {
        let input = r#"{"path": "/tmp/test.txt",}"#;
        let result = parse_tool_input(input);
        assert_eq!(result["path"], "/tmp/test.txt");
    }

    #[test]
    fn test_convert_user_text_message() {
        let msg = Message::user("hello");
        let items = convert_messages(&[msg]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["role"], "user");
        assert_eq!(items[0]["content"], "hello");
    }

    #[test]
    fn test_convert_assistant_text_message() {
        let msg = Message::assistant("hi there");
        let items = convert_messages(&[msg]);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["role"], "assistant");
        assert_eq!(items[0]["content"][0]["type"], "output_text");
        assert_eq!(items[0]["content"][0]["text"], "hi there");
    }

    #[test]
    fn test_convert_tool_use_message() {
        let msg = Message::assistant_blocks(vec![
            ContentBlock::Text {
                text: "Let me read that file.".to_string(),
            },
            ContentBlock::ToolUse {
                id: "call_123".to_string(),
                name: "ReadFiles".to_string(),
                input: json!({"path": "/tmp/test.txt"}),
            },
        ]);
        let items = convert_messages(&[msg]);
        assert_eq!(items.len(), 2);
        // First item: text
        assert_eq!(items[0]["role"], "assistant");
        assert_eq!(items[0]["content"][0]["text"], "Let me read that file.");
        // Second item: function_call
        assert_eq!(items[1]["type"], "function_call");
        assert_eq!(items[1]["call_id"], "call_123");
        assert_eq!(items[1]["name"], "ReadFiles");
    }

    #[test]
    fn test_convert_tool_result_message() {
        let msg = Message::tool_result("call_123", "file contents here", false);
        let items = convert_messages(&[msg]);
        // Tool results from User messages with ToolResult blocks
        assert!(!items.is_empty());
        let last = items.last().unwrap();
        assert_eq!(last["type"], "function_call_output");
        assert_eq!(last["call_id"], "call_123");
        assert_eq!(last["output"], "file contents here");
    }

    #[test]
    fn test_codex_models_list() {
        let models = codex_models();
        assert!(models.contains(&"gpt-5.4".to_string()));
        assert!(models.contains(&"gpt-5.3-codex".to_string()));
        assert_eq!(models.len(), 12);
    }
}
