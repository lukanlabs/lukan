use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use lukan_core::models::events::{StopReason, StreamEvent};
use lukan_core::models::messages::{ContentBlock, ImageSource, Message, MessageContent, Role};
use lukan_core::models::tools::ToolDefinition;

use crate::contracts::{Provider, StreamParams, SystemPrompt};
use crate::sse::{SseEvent, SseParser};

const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            max_tokens,
        }
    }

    /// Build system blocks with cache_control for Anthropic's prompt caching
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
                            // Single cache breakpoint on last cached block —
                            // everything before it gets cached as a prefix.
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

    /// Convert our canonical messages to Anthropic format
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
                    // Tool results wrapped in user message with tool_result blocks
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

    /// Convert content blocks to Anthropic format
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
                ContentBlock::Thinking { .. } => {
                    // Skip thinking blocks — Claude API doesn't accept them back
                }
            }
        }

        result
    }

    /// Convert tool definitions to Anthropic format
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
                // Cache breakpoint on last tool only
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
impl Provider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
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

        debug!("Sending request to Anthropic API (model: {})", self.model);

        let response = self
            .client
            .post(ANTHROPIC_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to connect to Anthropic API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error ({}): {}", status, error_body);
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

        let chunk_timeout = std::time::Duration::from_secs(60);
        while let Some(chunk) = tokio::time::timeout(chunk_timeout, stream.next())
            .await
            .context("Provider stream timed out (no data for 60s)")?
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
                                warn!("Failed to parse Anthropic SSE event: {err}");
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
                                    output_tokens = u.output_tokens.unwrap_or(0);
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

                            AnthropicStreamEvent::MessageStop => {
                                // No action needed — stop_reason is on MessageDelta
                            }

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

// ── Anthropic SSE event types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicStreamEvent {
    MessageStart {
        message: AnthropicMessage,
    },
    ContentBlockStart {
        index: u32,
        content_block: AnthropicContentBlock,
    },
    ContentBlockDelta {
        index: u32,
        delta: AnthropicDelta,
    },
    ContentBlockStop {
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

// ── Model listing ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicModel {
    pub id: String,
    pub display_name: String,
    pub created_at: String,
}

/// Fetch available models from the Anthropic API (paginated)
pub async fn fetch_anthropic_models(api_key: &str) -> Result<Vec<AnthropicModel>> {
    let client = Client::new();
    let mut models = Vec::new();
    let mut after_id: Option<String> = None;

    loop {
        let mut url = url::Url::parse("https://api.anthropic.com/v1/models")?;
        url.query_pairs_mut().append_pair("limit", "100");
        if let Some(ref id) = after_id {
            url.query_pairs_mut().append_pair("after_id", id);
        }

        let resp = client
            .get(url)
            .header("x-api-key", api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Failed to fetch Anthropic models: {status} {body}");
        }

        let page: AnthropicModelsPage = resp.json().await?;
        models.extend(page.data.into_iter().map(|m| AnthropicModel {
            id: m.id,
            display_name: m.display_name,
            created_at: m.created_at,
        }));

        if !page.has_more {
            break;
        }
        after_id = Some(page.last_id);
    }

    Ok(models)
}

#[derive(Debug, Deserialize)]
struct AnthropicModelsPage {
    data: Vec<AnthropicModelRaw>,
    has_more: bool,
    last_id: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicModelRaw {
    id: String,
    display_name: String,
    created_at: String,
}
