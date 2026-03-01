//! Shared base for OpenAI-compatible providers (Nebius, Fireworks, GitHub Copilot, z.ai, etc.)
//!
//! Handles message/tool conversion and streaming for the standard
//! OpenAI Chat Completions API format (`POST /chat/completions` with `stream: true`).

use std::collections::HashMap;

use anyhow::{Context, Result};
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use lukan_core::models::events::{StopReason, StreamEvent};
use lukan_core::models::messages::{ContentBlock, ImageSource, Message, MessageContent, Role};
use lukan_core::models::tools::ToolDefinition;

use crate::contracts::{StreamParams, SystemPrompt};
use crate::schema_adapter::strip_schema_keys;
use crate::sse::{SseEvent, SseParser};
use crate::think_tag_parser::{ThinkTagOutput, ThinkTagParser};

/// Schema keywords stripped for providers that don't support strict JSON schema.
const STRIP_KEYWORDS: &[&str] = &[
    "minItems",
    "maxItems",
    "minimum",
    "maximum",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "minLength",
    "maxLength",
    "pattern",
    "format",
];

/// Normalize an OpenAI-compatible base URL by stripping endpoint path suffixes.
///
/// Users often paste full endpoint URLs like `http://localhost:8080/v1/chat/completions`
/// when only the base `http://localhost:8080/v1` is needed. This function strips the
/// endpoint portion while preserving the `/v1` prefix that most servers require.
///
/// Examples:
/// - `http://host:8080/v1/chat/completions` → `http://host:8080/v1`
/// - `http://host:8080/chat/completions`     → `http://host:8080`
/// - `http://host:8080/v1`                   → `http://host:8080/v1` (unchanged)
pub fn normalize_base_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return trimmed.to_string();
    }

    // Endpoint-only suffixes (no /v1 prefix) — strip entirely
    const ENDPOINT_SUFFIXES: &[&str] = &[
        "/chat/completions",
        "/completions",
        "/responses",
        "/messages",
    ];

    // /v1/* suffixes — strip the endpoint part but keep /v1
    for suffix in ENDPOINT_SUFFIXES {
        let v1_suffix = format!("/v1{suffix}");
        if trimmed.ends_with(&v1_suffix) {
            return format!("{}/v1", &trimmed[..trimmed.len() - v1_suffix.len()]);
        }
    }

    // Bare endpoint suffixes (servers without /v1 prefix)
    for suffix in ENDPOINT_SUFFIXES {
        if let Some(base) = trimmed.strip_suffix(suffix) {
            return base.to_string();
        }
    }

    trimmed.to_string()
}

/// Model info returned from the /models endpoint.
#[derive(Debug, Deserialize)]
struct OpenAiModelEntry {
    id: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Option<Vec<OpenAiModelEntry>>,
}

/// Fetch available models from an OpenAI-compatible `/models` endpoint.
pub async fn fetch_openai_compatible_models(base_url: &str, api_key: &str) -> Result<Vec<String>> {
    let base = normalize_base_url(base_url);
    let url = format!("{}/models", base.trim_end_matches('/'));

    let client = Client::new();
    let mut req = client
        .get(&url)
        .header("accept", "application/json")
        .timeout(std::time::Duration::from_secs(15));

    if !api_key.is_empty() {
        req = req.header("authorization", format!("Bearer {api_key}"));
    }

    let resp = req
        .send()
        .await
        .with_context(|| format!("Failed to connect to {url}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("OpenAI-compatible models API error: {status} {body}");
    }

    let data: OpenAiModelsResponse = resp
        .json()
        .await
        .context("Failed to parse models response")?;
    let models = data
        .data
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.id)
        .collect();

    Ok(models)
}

/// Configuration for an OpenAI-compatible provider.
pub struct OpenAiCompatConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub extra_headers: HashMap<String, String>,
    /// Whether to parse `<think>` tags from text deltas (DeepSeek-style reasoning).
    pub use_think_tags: bool,
    /// Whether to strip advanced schema keywords (for servers that don't support them).
    pub strip_schema: bool,
    /// Whether this model supports image inputs.
    pub supports_images: bool,
}

/// Shared OpenAI-compatible streaming implementation.
///
/// Individual providers (Nebius, Fireworks, etc.) compose this struct
/// and delegate their `Provider::stream` call to it.
pub struct OpenAiCompatBase {
    client: Client,
    pub config: OpenAiCompatConfig,
}

impl OpenAiCompatBase {
    pub fn new(config: OpenAiCompatConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    /// Build the system prompt as a single string.
    fn build_system_content(prompt: &SystemPrompt) -> String {
        match prompt {
            SystemPrompt::Text(text) => text.clone(),
            SystemPrompt::Structured { cached, dynamic } => {
                let mut parts: Vec<&str> = cached.iter().map(|s| s.as_str()).collect();
                if !dynamic.is_empty() {
                    parts.push(dynamic);
                }
                parts.join("\n\n")
            }
        }
    }

    /// Convert our canonical messages to OpenAI format.
    fn convert_messages(
        &self,
        system_content: &str,
        messages: &[Message],
    ) -> Vec<serde_json::Value> {
        let mut result = Vec::new();

        // System message first
        result.push(serde_json::json!({
            "role": "system",
            "content": system_content
        }));

        for msg in messages {
            match msg.role {
                Role::User => {
                    let content = match &msg.content {
                        MessageContent::Text(s) => serde_json::json!(s),
                        MessageContent::Blocks(blocks) => {
                            let parts = self.convert_user_blocks(blocks);
                            // If only tool_result blocks, emit as separate tool messages
                            if parts.is_empty() {
                                // All blocks were tool results — handled separately
                                for block in blocks {
                                    if let ContentBlock::ToolResult {
                                        tool_use_id,
                                        content,
                                        ..
                                    } = block
                                    {
                                        result.push(serde_json::json!({
                                            "role": "tool",
                                            "tool_call_id": tool_use_id,
                                            "content": content
                                        }));
                                    }
                                }
                                continue;
                            }
                            serde_json::json!(parts)
                        }
                    };
                    result.push(serde_json::json!({ "role": "user", "content": content }));
                }
                Role::Assistant => {
                    let mut text_parts = Vec::new();
                    let mut tool_calls = Vec::new();

                    match &msg.content {
                        MessageContent::Text(s) => {
                            text_parts.push(s.clone());
                        }
                        MessageContent::Blocks(blocks) => {
                            for block in blocks {
                                match block {
                                    ContentBlock::Text { text } => {
                                        if !text.is_empty() {
                                            text_parts.push(text.clone());
                                        }
                                    }
                                    ContentBlock::ToolUse { id, name, input } => {
                                        tool_calls.push(serde_json::json!({
                                            "id": id,
                                            "type": "function",
                                            "function": {
                                                "name": name,
                                                "arguments": serde_json::to_string(input).unwrap_or_default()
                                            }
                                        }));
                                    }
                                    ContentBlock::Thinking { .. } => {
                                        // Skip thinking blocks
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    let mut assistant_msg = serde_json::json!({
                        "role": "assistant",
                        "content": if text_parts.is_empty() { serde_json::Value::Null } else { serde_json::json!(text_parts.join("\n")) }
                    });

                    if !tool_calls.is_empty() {
                        assistant_msg["tool_calls"] = serde_json::json!(tool_calls);
                    }

                    result.push(assistant_msg);
                }
                Role::Tool => {
                    let tool_call_id = msg.tool_call_id.as_deref().unwrap_or("");
                    let content_text = msg.content.to_text();
                    result.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tool_call_id,
                        "content": content_text
                    }));
                }
            }
        }

        result
    }

    /// Convert user content blocks to OpenAI multimodal format.
    /// Returns empty vec if all blocks are tool results (they're handled separately).
    fn convert_user_blocks(&self, blocks: &[ContentBlock]) -> Vec<serde_json::Value> {
        let mut parts = Vec::new();
        let mut has_non_tool = false;

        for block in blocks {
            match block {
                ContentBlock::Text { text } => {
                    if !text.is_empty() {
                        parts.push(serde_json::json!({ "type": "text", "text": text }));
                        has_non_tool = true;
                    }
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
                    parts.push(serde_json::json!({
                        "type": "image_url",
                        "image_url": { "url": url }
                    }));
                    has_non_tool = true;
                }
                ContentBlock::ToolResult { .. } => {
                    // Handled separately as tool messages
                }
                _ => {}
            }
        }

        if has_non_tool { parts } else { vec![] }
    }

    /// Convert tool definitions to OpenAI function calling format.
    fn convert_tools(&self, tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|t| {
                let schema = if self.config.strip_schema {
                    strip_schema_keys(&t.input_schema, STRIP_KEYWORDS)
                } else {
                    t.input_schema.clone()
                };

                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": schema
                    }
                })
            })
            .collect()
    }

    /// Stream a response using the OpenAI Chat Completions API.
    pub async fn stream(&self, params: StreamParams, tx: mpsc::Sender<StreamEvent>) -> Result<()> {
        let system_content = Self::build_system_content(&params.system_prompt);
        let messages = self.convert_messages(&system_content, &params.messages);
        let tools = self.convert_tools(&params.tools);

        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
            "stream": true,
            "max_tokens": self.config.max_tokens,
            "stream_options": { "include_usage": true }
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
        }

        debug!("Sending request to {} (model: {})", url, self.config.model);

        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", self.config.api_key));

        for (key, value) in &self.config.extra_headers {
            req = req.header(key.as_str(), value.as_str());
        }

        let response = req
            .json(&body)
            .send()
            .await
            .with_context(|| format!("Failed to connect to {url}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            anyhow::bail!("API error ({}): {}", status, error_body);
        }

        tx.send(StreamEvent::MessageStart).await.ok();

        let mut sse_parser = SseParser::new();
        let mut think_parser = if self.config.use_think_tags {
            Some(ThinkTagParser::new())
        } else {
            None
        };

        // Track parallel tool calls by index
        let mut tool_calls: HashMap<u32, ToolCallAccum> = HashMap::new();
        let mut input_tokens: u64 = 0;
        let mut output_tokens: u64 = 0;
        let mut cached_tokens: Option<u64> = None;
        let mut finish_reason: Option<String> = None;

        let mut stream = response.bytes_stream();
        use futures::StreamExt;

        let chunk_timeout = std::time::Duration::from_secs(60);
        let mut stream_done = false;
        while let Some(chunk) = tokio::time::timeout(chunk_timeout, stream.next())
            .await
            .context("Provider stream timed out (no data for 60s)")?
        {
            let chunk = chunk.context("Error reading stream chunk")?;
            let text = String::from_utf8_lossy(&chunk);

            for sse_event in sse_parser.feed(&text) {
                match sse_event {
                    SseEvent::Done => {
                        stream_done = true;
                        break;
                    }
                    SseEvent::Data(data) => {
                        let chunk: OpenAiChunk = match serde_json::from_str(&data) {
                            Ok(c) => c,
                            Err(err) => {
                                warn!("Failed to parse OpenAI SSE chunk: {err}");
                                continue;
                            }
                        };

                        // Usage (may appear in final chunk)
                        if let Some(usage) = chunk.usage {
                            input_tokens = usage.prompt_tokens.unwrap_or(0);
                            output_tokens = usage.completion_tokens.unwrap_or(0);
                            cached_tokens = usage
                                .prompt_tokens_details
                                .and_then(|d| d.cached_tokens)
                                .filter(|&v| v > 0);
                        }

                        let Some(choice) = chunk.choices.first() else {
                            continue;
                        };

                        if let Some(ref reason) = choice.finish_reason {
                            finish_reason = Some(reason.clone());
                        }

                        let delta = &choice.delta;

                        // Reasoning/thinking content (Ollama reasoning models, DeepSeek R1)
                        if let Some(ref reasoning) = delta.reasoning_content
                            && !reasoning.is_empty()
                        {
                            tx.send(StreamEvent::ThinkingDelta {
                                text: reasoning.clone(),
                            })
                            .await
                            .ok();
                        }

                        // Text content
                        if let Some(ref content) = delta.content
                            && !content.is_empty()
                        {
                            if let Some(ref mut tp) = think_parser {
                                for output in tp.feed(content) {
                                    match output {
                                        ThinkTagOutput::Text(t) => {
                                            tx.send(StreamEvent::TextDelta { text: t }).await.ok();
                                        }
                                        ThinkTagOutput::Thinking(t) => {
                                            tx.send(StreamEvent::ThinkingDelta { text: t })
                                                .await
                                                .ok();
                                        }
                                    }
                                }
                            } else {
                                tx.send(StreamEvent::TextDelta {
                                    text: content.clone(),
                                })
                                .await
                                .ok();
                            }
                        }

                        // Tool calls
                        if let Some(ref tc_deltas) = delta.tool_calls {
                            for tc in tc_deltas {
                                let idx = tc.index;
                                let entry =
                                    tool_calls.entry(idx).or_insert_with(|| ToolCallAccum {
                                        id: String::new(),
                                        name: String::new(),
                                        arguments: String::new(),
                                        started: false,
                                    });

                                if let Some(ref id) = tc.id {
                                    entry.id = id.clone();
                                }

                                if let Some(ref func) = tc.function {
                                    if let Some(ref name) = func.name {
                                        entry.name = name.clone();
                                    }
                                    if let Some(ref args) = func.arguments {
                                        entry.arguments.push_str(args);
                                    }
                                }

                                // Emit ToolUseStart once we have id + name
                                if !entry.started && !entry.id.is_empty() && !entry.name.is_empty()
                                {
                                    entry.started = true;
                                    tx.send(StreamEvent::ToolUseStart {
                                        id: entry.id.clone(),
                                        name: entry.name.clone(),
                                    })
                                    .await
                                    .ok();
                                }

                                // Emit argument delta
                                if let Some(ref func) = tc.function
                                    && let Some(ref args) = func.arguments
                                    && !args.is_empty()
                                {
                                    tx.send(StreamEvent::ToolUseDelta {
                                        input: args.clone(),
                                    })
                                    .await
                                    .ok();
                                }
                            }
                        }
                    }
                }
            }
            if stream_done {
                break;
            }
        }

        // Flush think tag parser
        if let Some(ref mut tp) = think_parser
            && let Some(output) = tp.flush()
        {
            match output {
                ThinkTagOutput::Text(t) => {
                    tx.send(StreamEvent::TextDelta { text: t }).await.ok();
                }
                ThinkTagOutput::Thinking(t) => {
                    tx.send(StreamEvent::ThinkingDelta { text: t }).await.ok();
                }
            }
        }

        // Emit ToolUseEnd for all accumulated tool calls
        let mut sorted_calls: Vec<_> = tool_calls.into_iter().collect();
        sorted_calls.sort_by_key(|(idx, _)| *idx);

        for (_idx, tc) in sorted_calls {
            let parsed_input: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::json!({}));
            tx.send(StreamEvent::ToolUseEnd {
                id: tc.id,
                name: tc.name,
                input: parsed_input,
            })
            .await
            .ok();
        }

        // Emit usage and message end
        tx.send(StreamEvent::Usage {
            input_tokens,
            output_tokens,
            cache_creation_tokens: None,
            cache_read_tokens: cached_tokens,
        })
        .await
        .ok();

        let stop_reason = match finish_reason.as_deref() {
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        };

        tx.send(StreamEvent::MessageEnd { stop_reason }).await.ok();

        Ok(())
    }
}

/// Accumulator for a single tool call being streamed.
struct ToolCallAccum {
    id: String,
    name: String,
    arguments: String,
    started: bool,
}

// ── OpenAI streaming chunk types ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OpenAiChunk {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    delta: OpenAiDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiDelta {
    #[serde(default)]
    content: Option<String>,
    /// Reasoning/thinking content (Ollama reasoning models, DeepSeek R1, etc.)
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCallDelta {
    index: u32,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OpenAiFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct OpenAiFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    #[serde(default)]
    prompt_tokens_details: Option<OpenAiPromptTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct OpenAiPromptTokensDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
}
