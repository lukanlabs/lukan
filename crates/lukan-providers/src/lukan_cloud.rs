//! Lukan Cloud provider — sends requests to the lukan-cloud proxy which
//! transparently forwards them to the upstream LLM (Anthropic).
//!
//! The proxy accepts the full Anthropic-format body (system, messages, tools)
//! and streams back raw Anthropic SSE events. This provider reuses the same
//! Anthropic request-building and SSE-parsing logic.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use lukan_core::models::events::{StopReason, StreamEvent};
use lukan_core::models::messages::{ContentBlock, ImageSource, Message, MessageContent, Role};
use lukan_core::models::tools::ToolDefinition;

use crate::contracts::{Provider, StreamParams, SystemPrompt};
use crate::sse::{SseEvent, SseParser};

use serde::Deserialize;

const LUKAN_CLOUD_BASE_URL: &str = "https://api.lukan.ai";

// ── Model fetching ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LukanCloudModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub tier: String,
}

pub async fn fetch_lukan_cloud_models(api_key: &str) -> Result<Vec<LukanCloudModel>> {
    let url = format!("{}/v1/models", LUKAN_CLOUD_BASE_URL);
    let client = Client::new();
    let resp = client
        .get(&url)
        .header("authorization", format!("Bearer {api_key}"))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .context("Failed to fetch Lukan Cloud models")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Lukan Cloud models API error: {status} {body}");
    }

    let models: Vec<LukanCloudModel> = resp.json().await?;
    Ok(models)
}

pub struct LukanCloudProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
}

impl LukanCloudProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: LUKAN_CLOUD_BASE_URL.to_string(),
            model,
            max_tokens,
        }
    }

    fn build_system_blocks(&self, prompt: &SystemPrompt) -> Vec<serde_json::Value> {
        match prompt {
            SystemPrompt::Text(text) => {
                vec![serde_json::json!({ "type": "text", "text": text })]
            }
            SystemPrompt::Structured { cached, dynamic } => {
                let last_cached = cached.len().saturating_sub(1);
                let mut blocks: Vec<serde_json::Value> = cached
                    .iter()
                    .enumerate()
                    .map(|(i, text)| {
                        if i == last_cached {
                            serde_json::json!({
                                "type": "text",
                                "text": text,
                                "cache_control": { "type": "ephemeral" }
                            })
                        } else {
                            serde_json::json!({ "type": "text", "text": text })
                        }
                    })
                    .collect();

                if !dynamic.is_empty() {
                    blocks.push(serde_json::json!({ "type": "text", "text": dynamic }));
                }

                blocks
            }
        }
    }

    fn convert_messages(&self, messages: &[Message]) -> Vec<serde_json::Value> {
        let mut result = Vec::new();

        for msg in messages {
            match msg.role {
                Role::User => {
                    let content = match &msg.content {
                        MessageContent::Text(s) => serde_json::json!(s),
                        MessageContent::Blocks(blocks) => {
                            serde_json::json!(self.convert_content_blocks(blocks))
                        }
                    };
                    result.push(serde_json::json!({ "role": "user", "content": content }));
                }
                Role::Assistant => {
                    let content = match &msg.content {
                        MessageContent::Text(s) => serde_json::json!(s),
                        MessageContent::Blocks(blocks) => {
                            serde_json::json!(self.convert_content_blocks(blocks))
                        }
                    };
                    result.push(serde_json::json!({ "role": "assistant", "content": content }));
                }
                Role::Tool => {
                    let tool_use_id = msg.tool_call_id.as_deref().unwrap_or("");
                    let content_text = msg.content.to_text();
                    result.push(serde_json::json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": content_text
                        }]
                    }));
                }
            }
        }

        result
    }

    fn convert_content_blocks(&self, blocks: &[ContentBlock]) -> Vec<serde_json::Value> {
        let mut result = Vec::new();

        for block in blocks {
            match block {
                ContentBlock::Text { text } => {
                    if !text.is_empty() {
                        result.push(serde_json::json!({ "type": "text", "text": text }));
                    }
                }
                ContentBlock::ToolUse { id, name, input } => {
                    result.push(serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input
                    }));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    ..
                } => {
                    let mut block_json = serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content
                    });
                    if let Some(true) = is_error {
                        block_json["is_error"] = serde_json::json!(true);
                    }
                    result.push(block_json);
                }
                ContentBlock::Image {
                    source,
                    data,
                    media_type,
                } => match source {
                    ImageSource::Url => {
                        result.push(serde_json::json!({
                            "type": "image",
                            "source": { "type": "url", "url": data }
                        }));
                    }
                    ImageSource::Base64 => {
                        result.push(serde_json::json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": media_type.as_deref().unwrap_or("image/jpeg"),
                                "data": data
                            }
                        }));
                    }
                },
                ContentBlock::Thinking { .. } => {}
            }
        }

        result
    }

    fn convert_tools(&self, tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut tool = serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema
                });
                if i == tools.len() - 1 {
                    tool["cache_control"] = serde_json::json!({ "type": "ephemeral" });
                }
                tool
            })
            .collect()
    }

    fn map_stop_reason(reason: Option<&str>) -> StopReason {
        match reason {
            Some("end_turn") => StopReason::EndTurn,
            Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        }
    }
}

#[async_trait]
impl Provider for LukanCloudProvider {
    fn name(&self) -> &str {
        "lukan-cloud"
    }

    fn supports_images(&self) -> bool {
        true
    }

    async fn stream(&self, params: StreamParams, tx: mpsc::Sender<StreamEvent>) -> Result<()> {
        let system_blocks = self.build_system_blocks(&params.system_prompt);
        let messages = self.convert_messages(&params.messages);
        let tools = self.convert_tools(&params.tools);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": system_blocks,
            "messages": messages,
            "stream": true
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
        }

        let url = format!("{}/v1/stream", self.base_url);
        debug!("Sending request to Lukan Cloud (model: {})", self.model);

        let response = self
            .client
            .post(&url)
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to connect to Lukan Cloud")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            if status.as_u16() == 402 {
                anyhow::bail!(
                    "Lukan Cloud quota exceeded. Upgrade your plan at https://lukan.cloud"
                );
            }
            anyhow::bail!("Lukan Cloud API error ({}): {}", status, error_body);
        }

        tx.send(StreamEvent::MessageStart).await.ok();

        let mut sse_parser = SseParser::new();
        let mut current_tool_id = String::new();
        let mut current_tool_name = String::new();
        let mut tool_input_json = String::new();
        let mut input_tokens: u64 = 0;
        let mut output_tokens: u64 = 0;
        let mut cache_creation_tokens: u64 = 0;
        let mut cache_read_tokens: u64 = 0;

        let mut stream = response.bytes_stream();
        use futures::StreamExt;

        let chunk_timeout = std::time::Duration::from_secs(120);
        while let Some(chunk) = tokio::time::timeout(chunk_timeout, stream.next())
            .await
            .context("Lukan Cloud stream timed out (no data for 60s)")?
        {
            let chunk = chunk.context("Error reading stream chunk")?;
            let text = String::from_utf8_lossy(&chunk);

            for sse_event in sse_parser.feed(&text) {
                match sse_event {
                    SseEvent::Done => break,
                    SseEvent::Data(data) => {
                        let event: AnthropicStreamEvent = match serde_json::from_str(&data) {
                            Ok(e) => e,
                            Err(err) => {
                                warn!("Failed to parse SSE event: {err}");
                                continue;
                            }
                        };

                        match event {
                            AnthropicStreamEvent::MessageStart { message } => {
                                if let Some(usage) = message.usage {
                                    input_tokens = usage.input_tokens.unwrap_or(0);
                                    cache_creation_tokens =
                                        usage.cache_creation_input_tokens.unwrap_or(0);
                                    cache_read_tokens = usage.cache_read_input_tokens.unwrap_or(0);
                                }
                            }

                            AnthropicStreamEvent::ContentBlockStart { content_block, .. } => {
                                if content_block.r#type == "tool_use" {
                                    current_tool_id = content_block.id.unwrap_or_default();
                                    current_tool_name = content_block.name.unwrap_or_default();
                                    tool_input_json.clear();
                                    tx.send(StreamEvent::ToolUseStart {
                                        id: current_tool_id.clone(),
                                        name: current_tool_name.clone(),
                                    })
                                    .await
                                    .ok();
                                }
                            }

                            AnthropicStreamEvent::ContentBlockDelta { delta, .. } => {
                                match delta.r#type.as_str() {
                                    "text_delta" => {
                                        if let Some(text) = delta.text {
                                            tx.send(StreamEvent::TextDelta { text }).await.ok();
                                        }
                                    }
                                    "input_json_delta" => {
                                        if let Some(partial) = delta.partial_json {
                                            tool_input_json.push_str(&partial);
                                            tx.send(StreamEvent::ToolUseDelta { input: partial })
                                                .await
                                                .ok();
                                        }
                                    }
                                    _ => {}
                                }
                            }

                            AnthropicStreamEvent::ContentBlockStop { .. } => {
                                if !current_tool_id.is_empty() {
                                    let parsed_input: serde_json::Value =
                                        serde_json::from_str(&tool_input_json)
                                            .unwrap_or(serde_json::json!({}));
                                    tx.send(StreamEvent::ToolUseEnd {
                                        id: current_tool_id.clone(),
                                        name: current_tool_name.clone(),
                                        input: parsed_input,
                                    })
                                    .await
                                    .ok();
                                    current_tool_id.clear();
                                    current_tool_name.clear();
                                    tool_input_json.clear();
                                }
                            }

                            AnthropicStreamEvent::MessageDelta { delta, usage } => {
                                if let Some(u) = usage {
                                    output_tokens = u.output_tokens.unwrap_or(output_tokens);
                                    if let Some(it) = u.input_tokens {
                                        input_tokens = it;
                                    }
                                    if let Some(cr) = u.cache_read_input_tokens {
                                        cache_read_tokens = cr;
                                    }
                                }
                                let stop_reason =
                                    Self::map_stop_reason(delta.stop_reason.as_deref());

                                tx.send(StreamEvent::Usage {
                                    input_tokens,
                                    output_tokens,
                                    cache_creation_tokens: if cache_creation_tokens > 0 {
                                        Some(cache_creation_tokens)
                                    } else {
                                        None
                                    },
                                    cache_read_tokens: if cache_read_tokens > 0 {
                                        Some(cache_read_tokens)
                                    } else {
                                        None
                                    },
                                })
                                .await
                                .ok();

                                tx.send(StreamEvent::MessageEnd { stop_reason }).await.ok();
                            }

                            AnthropicStreamEvent::MessageStop => {}

                            AnthropicStreamEvent::Ping => {}

                            AnthropicStreamEvent::Error { error } => {
                                tx.send(StreamEvent::Error {
                                    error: format!("{}: {}", error.r#type, error.message),
                                })
                                .await
                                .ok();
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

// ── Anthropic SSE event types (same as anthropic.rs) ─────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicStreamEvent {
    MessageStart {
        message: AnthropicMessage,
    },
    ContentBlockStart {
        #[allow(dead_code)]
        index: u32,
        content_block: AnthropicContentBlock,
    },
    ContentBlockDelta {
        #[allow(dead_code)]
        index: u32,
        delta: AnthropicDelta,
    },
    ContentBlockStop {
        #[allow(dead_code)]
        index: u32,
    },
    MessageDelta {
        delta: AnthropicMessageDelta,
        #[serde(default)]
        usage: Option<AnthropicOutputUsage>,
    },
    MessageStop,
    Ping,
    Error {
        error: AnthropicApiError,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicMessage {
    usage: Option<AnthropicInputUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicInputUsage {
    input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AnthropicOutputUsage {
    output_tokens: Option<u64>,
    /// Some proxies (e.g. lukan-cloud translating Cloudflare AI Gateway streams)
    /// deliver the real prompt-token count in `message_delta` because the upstream
    /// sends it in chunks that arrive after the initial `message_start`.
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AnthropicContentBlock {
    r#type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    r#type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicMessageDelta {
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnthropicApiError {
    r#type: String,
    message: String,
}
