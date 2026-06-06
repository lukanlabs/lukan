use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use lukan_core::models::events::{StopReason, StreamEvent};
use lukan_core::models::messages::{ContentBlock, Message, MessageContent, Role};
use lukan_core::models::tools::ToolDefinition;

use crate::contracts::{CachePolicy, Provider, StreamParams, SystemPrompt};
use crate::sse::{SseEvent, SseParser};

const MINIMAX_API_URL: &str = "https://api.minimax.io/anthropic/v1/messages";
const MINIMAX_ANTHROPIC_VERSION: &str = "2023-06-01";

const MINIMAX_MODELS: &[(&str, &str)] = &[
    ("MiniMax-M3", "MiniMax M3"),
    ("MiniMax-M2.7", "MiniMax M2.7"),
    ("MiniMax-M2.7-highspeed", "MiniMax M2.7 Highspeed"),
    ("MiniMax-M2.5", "MiniMax M2.5"),
    ("MiniMax-M2.5-highspeed", "MiniMax M2.5 Highspeed"),
    ("MiniMax-M2.1", "MiniMax M2.1"),
    ("MiniMax-M2.1-highspeed", "MiniMax M2.1 Highspeed"),
    ("MiniMax-M2", "MiniMax M2"),
];

pub struct MiniMaxProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
}

impl MiniMaxProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            max_tokens,
        }
    }

    fn build_system_blocks(&self, prompt: &SystemPrompt) -> Vec<serde_json::Value> {
        match prompt {
            SystemPrompt::Text(text) => vec![serde_json::json!({ "type": "text", "text": text })],
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

    fn convert_messages(
        &self,
        messages: &[Message],
        cache_policy: &CachePolicy,
    ) -> Vec<serde_json::Value> {
        let mut result = Vec::new();

        for (idx, msg) in messages.iter().enumerate() {
            let should_cache_message = cache_policy.message_breakpoint == Some(idx);
            match msg.role {
                Role::User => {
                    let content = match &msg.content {
                        MessageContent::Text(s) => {
                            if should_cache_message {
                                serde_json::json!([{ "type": "text", "text": s, "cache_control": { "type": "ephemeral" } }])
                            } else {
                                serde_json::json!(s)
                            }
                        }
                        MessageContent::Blocks(blocks) => serde_json::json!(
                            self.convert_content_blocks(blocks, should_cache_message)
                        ),
                    };
                    result.push(serde_json::json!({ "role": "user", "content": content }));
                }
                Role::Assistant => {
                    let content = match &msg.content {
                        MessageContent::Text(s) => {
                            if should_cache_message {
                                serde_json::json!([{ "type": "text", "text": s, "cache_control": { "type": "ephemeral" } }])
                            } else {
                                serde_json::json!(s)
                            }
                        }
                        MessageContent::Blocks(blocks) => serde_json::json!(
                            self.convert_content_blocks(blocks, should_cache_message)
                        ),
                    };
                    result.push(serde_json::json!({ "role": "assistant", "content": content }));
                }
                Role::Tool => {
                    let tool_use_id = msg.tool_call_id.as_deref().unwrap_or("");
                    let content_text = msg.content.to_text();
                    let mut tool_result = serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content_text
                    });
                    if should_cache_message {
                        tool_result["cache_control"] = serde_json::json!({ "type": "ephemeral" });
                    }
                    result.push(serde_json::json!({
                        "role": "user",
                        "content": [tool_result]
                    }));
                }
            }
        }

        result
    }

    fn convert_content_blocks(
        &self,
        blocks: &[ContentBlock],
        cache_last_block: bool,
    ) -> Vec<serde_json::Value> {
        let mut result = Vec::new();

        for (idx, block) in blocks.iter().enumerate() {
            let should_cache_block = cache_last_block && idx == blocks.len().saturating_sub(1);
            match block {
                ContentBlock::Text { text } => {
                    if !text.is_empty() {
                        let mut block_json = serde_json::json!({ "type": "text", "text": text });
                        if should_cache_block {
                            block_json["cache_control"] =
                                serde_json::json!({ "type": "ephemeral" });
                        }
                        result.push(block_json);
                    }
                }
                ContentBlock::ToolUse { id, name, input } => {
                    let mut block_json = serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input
                    });
                    if should_cache_block {
                        block_json["cache_control"] = serde_json::json!({ "type": "ephemeral" });
                    }
                    result.push(block_json);
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
                    if should_cache_block {
                        block_json["cache_control"] = serde_json::json!({ "type": "ephemeral" });
                    }
                    result.push(block_json);
                }
                ContentBlock::Image { .. } => {
                    // MiniMax's Anthropic-compatible API does not support images yet.
                }
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
impl Provider for MiniMaxProvider {
    fn name(&self) -> &str {
        "minimax"
    }

    async fn stream(&self, params: StreamParams, tx: mpsc::Sender<StreamEvent>) -> Result<()> {
        let system_blocks = self.build_system_blocks(&params.system_prompt);
        let messages = self.convert_messages(&params.messages, &params.cache_policy);
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

        debug!("Sending request to MiniMax API (model: {})", self.model);

        let response = self
            .client
            .post(MINIMAX_API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", MINIMAX_ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to connect to MiniMax API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            anyhow::bail!("MiniMax API error ({}): {}", status, error_body);
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
            .context("Provider stream timed out (no data for 120s)")?
        {
            let chunk = chunk.context("Error reading MiniMax stream chunk")?;
            let text = String::from_utf8_lossy(&chunk);

            for sse_event in sse_parser.feed(&text) {
                match sse_event {
                    SseEvent::Done => break,
                    SseEvent::Data(data) => {
                        let event: MiniMaxStreamEvent = match serde_json::from_str(&data) {
                            Ok(e) => e,
                            Err(err) => {
                                warn!("Failed to parse MiniMax SSE event: {err}");
                                continue;
                            }
                        };

                        match event {
                            MiniMaxStreamEvent::MessageStart { message } => {
                                if let Some(usage) = message.usage {
                                    input_tokens = usage.input_tokens.unwrap_or(0);
                                    cache_creation_tokens =
                                        usage.cache_creation_input_tokens.unwrap_or(0);
                                    cache_read_tokens = usage.cache_read_input_tokens.unwrap_or(0);
                                }
                            }
                            MiniMaxStreamEvent::ContentBlockStart { content_block, .. } => {
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
                            MiniMaxStreamEvent::ContentBlockDelta { delta, .. } => {
                                match delta.r#type.as_str() {
                                    "text_delta" => {
                                        if let Some(text) = delta.text {
                                            tx.send(StreamEvent::TextDelta { text }).await.ok();
                                        }
                                    }
                                    "thinking_delta" => {
                                        if let Some(thinking) = delta.thinking {
                                            tx.send(StreamEvent::ThinkingDelta { text: thinking })
                                                .await
                                                .ok();
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
                            MiniMaxStreamEvent::ContentBlockStop { .. } => {
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
                            MiniMaxStreamEvent::MessageDelta { delta, usage } => {
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
                            MiniMaxStreamEvent::MessageStop => {}
                            MiniMaxStreamEvent::Ping => {}
                            MiniMaxStreamEvent::Error { error } => {
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

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MiniMaxStreamEvent {
    MessageStart {
        message: MiniMaxMessage,
    },
    ContentBlockStart {
        index: u32,
        content_block: MiniMaxContentBlock,
    },
    ContentBlockDelta {
        index: u32,
        delta: MiniMaxDelta,
    },
    ContentBlockStop {
        index: u32,
    },
    MessageDelta {
        delta: MiniMaxMessageDelta,
        #[serde(default)]
        usage: Option<MiniMaxOutputUsage>,
    },
    MessageStop,
    Ping,
    Error {
        error: MiniMaxApiError,
    },
}

#[derive(Debug, Deserialize)]
struct MiniMaxMessage {
    usage: Option<MiniMaxInputUsage>,
}

#[derive(Debug, Deserialize)]
struct MiniMaxInputUsage {
    input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct MiniMaxOutputUsage {
    output_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct MiniMaxContentBlock {
    r#type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MiniMaxDelta {
    r#type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MiniMaxMessageDelta {
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MiniMaxApiError {
    r#type: String,
    message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiniMaxModel {
    pub id: String,
    pub display_name: String,
}

pub fn minimax_models() -> Vec<MiniMaxModel> {
    MINIMAX_MODELS
        .iter()
        .map(|(id, display_name)| MiniMaxModel {
            id: (*id).to_string(),
            display_name: (*display_name).to_string(),
        })
        .collect()
}

pub async fn fetch_minimax_models(_api_key: &str) -> Result<Vec<MiniMaxModel>> {
    Ok(minimax_models())
}
