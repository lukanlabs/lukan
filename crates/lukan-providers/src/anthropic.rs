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

        let chunk_timeout = std::time::Duration::from_secs(120);
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

#[cfg(test)]
mod tests {
    use super::*;
    use lukan_core::models::messages::{ContentBlock, ImageSource, Message, MessageContent};
    use lukan_core::models::tools::ToolDefinition;
    use serde_json::json;

    fn make_provider() -> AnthropicProvider {
        AnthropicProvider::new("test-key".into(), "claude-test".into(), 4096)
    }

    // ── map_stop_reason ───────────────────────────────────────────────

    #[test]
    fn map_stop_reason_end_turn() {
        assert_eq!(
            AnthropicProvider::map_stop_reason(Some("end_turn")),
            StopReason::EndTurn
        );
    }

    #[test]
    fn map_stop_reason_tool_use() {
        assert_eq!(
            AnthropicProvider::map_stop_reason(Some("tool_use")),
            StopReason::ToolUse
        );
    }

    #[test]
    fn map_stop_reason_max_tokens() {
        assert_eq!(
            AnthropicProvider::map_stop_reason(Some("max_tokens")),
            StopReason::MaxTokens
        );
    }

    #[test]
    fn map_stop_reason_unknown_defaults_to_end_turn() {
        assert_eq!(
            AnthropicProvider::map_stop_reason(Some("unknown")),
            StopReason::EndTurn
        );
        assert_eq!(
            AnthropicProvider::map_stop_reason(None),
            StopReason::EndTurn
        );
    }

    // ── build_system_blocks ───────────────────────────────────────────

    #[test]
    fn build_system_blocks_text() {
        let p = make_provider();
        let blocks = p.build_system_blocks(&SystemPrompt::Text("Hello system".into()));
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "Hello system");
        assert!(blocks[0].get("cache_control").is_none());
    }

    #[test]
    fn build_system_blocks_structured() {
        let p = make_provider();
        let blocks = p.build_system_blocks(&SystemPrompt::Structured {
            cached: vec!["block1".into(), "block2".into()],
            dynamic: "dynamic content".into(),
        });
        assert_eq!(blocks.len(), 3);
        // First cached block: no cache_control
        assert!(blocks[0].get("cache_control").is_none());
        assert_eq!(blocks[0]["text"], "block1");
        // Last cached block: has cache_control
        assert_eq!(blocks[1]["cache_control"]["type"], "ephemeral");
        assert_eq!(blocks[1]["text"], "block2");
        // Dynamic block
        assert_eq!(blocks[2]["text"], "dynamic content");
        assert!(blocks[2].get("cache_control").is_none());
    }

    #[test]
    fn build_system_blocks_structured_empty_dynamic() {
        let p = make_provider();
        let blocks = p.build_system_blocks(&SystemPrompt::Structured {
            cached: vec!["only cached".into()],
            dynamic: String::new(),
        });
        // Empty dynamic should not produce a block
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["text"], "only cached");
        assert_eq!(blocks[0]["cache_control"]["type"], "ephemeral");
    }

    // ── convert_messages ──────────────────────────────────────────────

    #[test]
    fn convert_messages_user_text() {
        let p = make_provider();
        let msgs = vec![Message::user("Hello")];
        let result = p.convert_messages(&msgs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "user");
        assert_eq!(result[0]["content"], "Hello");
    }

    #[test]
    fn convert_messages_assistant_text() {
        let p = make_provider();
        let msgs = vec![Message::assistant("Response")];
        let result = p.convert_messages(&msgs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "assistant");
        assert_eq!(result[0]["content"], "Response");
    }

    #[test]
    fn convert_messages_tool_result() {
        let p = make_provider();
        let msgs = vec![Message {
            role: Role::Tool,
            content: MessageContent::Text("tool output".into()),
            tool_call_id: Some("call_123".into()),
            name: None,
        }];
        let result = p.convert_messages(&msgs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "user");
        let content = result[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "tool_result");
        assert_eq!(content[0]["tool_use_id"], "call_123");
        assert_eq!(content[0]["content"], "tool output");
    }

    // ── convert_content_blocks ────────────────────────────────────────

    #[test]
    fn convert_content_blocks_text() {
        let p = make_provider();
        let blocks = vec![ContentBlock::Text {
            text: "hello".into(),
        }];
        let result = p.convert_content_blocks(&blocks);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["type"], "text");
        assert_eq!(result[0]["text"], "hello");
    }

    #[test]
    fn convert_content_blocks_skips_empty_text() {
        let p = make_provider();
        let blocks = vec![ContentBlock::Text {
            text: String::new(),
        }];
        let result = p.convert_content_blocks(&blocks);
        assert!(result.is_empty());
    }

    #[test]
    fn convert_content_blocks_tool_use() {
        let p = make_provider();
        let blocks = vec![ContentBlock::ToolUse {
            id: "tool_1".into(),
            name: "bash".into(),
            input: json!({"command": "ls"}),
        }];
        let result = p.convert_content_blocks(&blocks);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["type"], "tool_use");
        assert_eq!(result[0]["id"], "tool_1");
        assert_eq!(result[0]["name"], "bash");
        assert_eq!(result[0]["input"]["command"], "ls");
    }

    #[test]
    fn convert_content_blocks_tool_result_with_error() {
        let p = make_provider();
        let blocks = vec![ContentBlock::ToolResult {
            tool_use_id: "tool_1".into(),
            content: "failed".into(),
            is_error: Some(true),
            diff: None,
            image: None,
        }];
        let result = p.convert_content_blocks(&blocks);
        assert_eq!(result[0]["type"], "tool_result");
        assert_eq!(result[0]["is_error"], true);
    }

    #[test]
    fn convert_content_blocks_tool_result_without_error() {
        let p = make_provider();
        let blocks = vec![ContentBlock::ToolResult {
            tool_use_id: "tool_1".into(),
            content: "ok".into(),
            is_error: None,
            diff: None,
            image: None,
        }];
        let result = p.convert_content_blocks(&blocks);
        assert!(result[0].get("is_error").is_none());
    }

    #[test]
    fn convert_content_blocks_image_url() {
        let p = make_provider();
        let blocks = vec![ContentBlock::Image {
            source: ImageSource::Url,
            data: "https://example.com/img.png".into(),
            media_type: None,
        }];
        let result = p.convert_content_blocks(&blocks);
        assert_eq!(result[0]["type"], "image");
        assert_eq!(result[0]["source"]["type"], "url");
        assert_eq!(result[0]["source"]["url"], "https://example.com/img.png");
    }

    #[test]
    fn convert_content_blocks_image_base64() {
        let p = make_provider();
        let blocks = vec![ContentBlock::Image {
            source: ImageSource::Base64,
            data: "abc123".into(),
            media_type: Some("image/png".into()),
        }];
        let result = p.convert_content_blocks(&blocks);
        assert_eq!(result[0]["source"]["type"], "base64");
        assert_eq!(result[0]["source"]["media_type"], "image/png");
        assert_eq!(result[0]["source"]["data"], "abc123");
    }

    #[test]
    fn convert_content_blocks_skips_thinking() {
        let p = make_provider();
        let blocks = vec![ContentBlock::Thinking {
            text: "reasoning".into(),
        }];
        let result = p.convert_content_blocks(&blocks);
        assert!(result.is_empty());
    }

    // ── convert_tools ─────────────────────────────────────────────────

    #[test]
    fn convert_tools_cache_on_last() {
        let p = make_provider();
        let tools = vec![
            ToolDefinition {
                name: "bash".into(),
                description: "Run commands".into(),
                input_schema: json!({"type": "object"}),
                deferred: false,
                read_only: false,
                search_hint: None,
            },
            ToolDefinition {
                name: "read".into(),
                description: "Read files".into(),
                input_schema: json!({"type": "object"}),
                deferred: false,
                read_only: false,
                search_hint: None,
            },
        ];
        let result = p.convert_tools(&tools);
        assert_eq!(result.len(), 2);
        // First tool: no cache_control
        assert!(result[0].get("cache_control").is_none());
        // Last tool: has cache_control
        assert_eq!(result[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn convert_tools_single_tool_gets_cache() {
        let p = make_provider();
        let tools = vec![ToolDefinition {
            name: "bash".into(),
            description: "Run".into(),
            input_schema: json!({}),
            deferred: false,
            read_only: false,
            search_hint: None,
        }];
        let result = p.convert_tools(&tools);
        assert_eq!(result[0]["cache_control"]["type"], "ephemeral");
    }

    // ── SSE type deserialization ──────────────────────────────────────

    #[test]
    fn deserialize_message_start() {
        let json_str = r#"{"type":"message_start","message":{"usage":{"input_tokens":100,"cache_creation_input_tokens":50,"cache_read_input_tokens":25}}}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(json_str).unwrap();
        match event {
            AnthropicStreamEvent::MessageStart { message } => {
                let usage = message.usage.unwrap();
                assert_eq!(usage.input_tokens, Some(100));
                assert_eq!(usage.cache_creation_input_tokens, Some(50));
                assert_eq!(usage.cache_read_input_tokens, Some(25));
            }
            _ => panic!("Expected MessageStart"),
        }
    }

    #[test]
    fn deserialize_content_block_start_tool_use() {
        let json_str = r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_123","name":"bash"}}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(json_str).unwrap();
        match event {
            AnthropicStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                assert_eq!(index, 0);
                assert_eq!(content_block.r#type, "tool_use");
                assert_eq!(content_block.id.unwrap(), "toolu_123");
                assert_eq!(content_block.name.unwrap(), "bash");
            }
            _ => panic!("Expected ContentBlockStart"),
        }
    }

    #[test]
    fn deserialize_content_block_delta_text() {
        let json_str = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(json_str).unwrap();
        match event {
            AnthropicStreamEvent::ContentBlockDelta { delta, .. } => {
                assert_eq!(delta.r#type, "text_delta");
                assert_eq!(delta.text.unwrap(), "Hello");
            }
            _ => panic!("Expected ContentBlockDelta"),
        }
    }

    #[test]
    fn deserialize_content_block_delta_input_json() {
        let json_str = r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"cmd\":"}}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(json_str).unwrap();
        match event {
            AnthropicStreamEvent::ContentBlockDelta { delta, .. } => {
                assert_eq!(delta.r#type, "input_json_delta");
                assert_eq!(delta.partial_json.unwrap(), "{\"cmd\":");
            }
            _ => panic!("Expected ContentBlockDelta"),
        }
    }

    #[test]
    fn deserialize_message_delta() {
        let json_str = r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(json_str).unwrap();
        match event {
            AnthropicStreamEvent::MessageDelta { delta, usage } => {
                assert_eq!(delta.stop_reason.unwrap(), "end_turn");
                assert_eq!(usage.unwrap().output_tokens, Some(42));
            }
            _ => panic!("Expected MessageDelta"),
        }
    }

    #[test]
    fn deserialize_ping() {
        let json_str = r#"{"type":"ping"}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(json_str).unwrap();
        assert!(matches!(event, AnthropicStreamEvent::Ping));
    }

    #[test]
    fn deserialize_error() {
        let json_str =
            r#"{"type":"error","error":{"type":"overloaded_error","message":"API is overloaded"}}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(json_str).unwrap();
        match event {
            AnthropicStreamEvent::Error { error } => {
                assert_eq!(error.r#type, "overloaded_error");
                assert_eq!(error.message, "API is overloaded");
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn deserialize_message_stop() {
        let json_str = r#"{"type":"message_stop"}"#;
        let event: AnthropicStreamEvent = serde_json::from_str(json_str).unwrap();
        assert!(matches!(event, AnthropicStreamEvent::MessageStop));
    }

    // ── AnthropicModel serialization ──────────────────────────────────

    #[test]
    fn anthropic_model_roundtrip() {
        let model = AnthropicModel {
            id: "claude-3-opus".into(),
            display_name: "Claude 3 Opus".into(),
            created_at: "2024-01-01".into(),
        };
        let json = serde_json::to_value(&model).unwrap();
        assert_eq!(json["id"], "claude-3-opus");
        let deserialized: AnthropicModel = serde_json::from_value(json).unwrap();
        assert_eq!(deserialized.id, "claude-3-opus");
    }
}
