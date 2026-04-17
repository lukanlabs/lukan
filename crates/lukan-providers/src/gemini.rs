use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use lukan_core::models::events::{StopReason, StreamEvent};
use lukan_core::models::messages::{ContentBlock, ImageSource, Message, MessageContent, Role};
use lukan_core::models::tools::ToolDefinition;

use crate::contracts::{Provider, StreamParams, SystemPrompt};
use crate::sse::{SseEvent, SseParser};

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

pub struct GeminiProvider {
    client: Client,
    api_key: String,
    model: String,
    max_tokens: u32,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: String, max_tokens: u32) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            max_tokens,
        }
    }

    /// Build the `systemInstruction` field from our SystemPrompt
    fn build_system_instruction(&self, prompt: &SystemPrompt) -> serde_json::Value {
        match prompt {
            SystemPrompt::Text(text) => {
                serde_json::json!({ "parts": [{ "text": text }] })
            }
            SystemPrompt::Structured { cached, dynamic } => {
                let mut parts: Vec<serde_json::Value> = cached
                    .iter()
                    .map(|text| serde_json::json!({ "text": text }))
                    .collect();
                if !dynamic.is_empty() {
                    parts.push(serde_json::json!({ "text": dynamic }));
                }
                serde_json::json!({ "parts": parts })
            }
        }
    }

    /// Convert canonical messages to Gemini `contents` array.
    ///
    /// Gemini uses `"user"` and `"model"` roles.
    /// Tool results are sent as `functionResponse` parts in a `"user"` turn.
    /// Consecutive same-role messages are merged into a single content entry.
    fn convert_messages(&self, messages: &[Message]) -> Vec<serde_json::Value> {
        let mut result: Vec<serde_json::Value> = Vec::new();

        for msg in messages {
            let (role, parts) = match msg.role {
                Role::User => ("user", self.convert_user_parts(&msg.content)),
                Role::Assistant => ("model", self.convert_assistant_parts(&msg.content)),
                Role::Tool => {
                    let name = msg.tool_call_id.as_deref().unwrap_or("unknown");
                    let content_text = msg.content.to_text();
                    // Try to parse as JSON for richer responses, fallback to string wrapper
                    let response_val = serde_json::from_str::<serde_json::Value>(&content_text)
                        .unwrap_or_else(|_| serde_json::json!({ "result": content_text }));
                    let parts = vec![serde_json::json!({
                        "functionResponse": {
                            "name": name,
                            "response": response_val
                        }
                    })];
                    ("user", parts)
                }
            };

            // Gemini requires alternating user/model turns.
            // Merge consecutive same-role messages into one content entry.
            if let Some(last) = result.last_mut()
                && last["role"].as_str() == Some(role)
                && let Some(existing_parts) = last["parts"].as_array_mut()
            {
                existing_parts.extend(parts);
                continue;
            }

            result.push(serde_json::json!({ "role": role, "parts": parts }));
        }

        result
    }

    fn convert_user_parts(&self, content: &MessageContent) -> Vec<serde_json::Value> {
        match content {
            MessageContent::Text(s) => vec![serde_json::json!({ "text": s })],
            MessageContent::Blocks(blocks) => {
                let mut parts = Vec::new();
                for block in blocks {
                    match block {
                        ContentBlock::Text { text } if !text.is_empty() => {
                            parts.push(serde_json::json!({ "text": text }));
                        }
                        ContentBlock::Image {
                            source,
                            data,
                            media_type,
                        } => match source {
                            ImageSource::Base64 => {
                                parts.push(serde_json::json!({
                                    "inline_data": {
                                        "mime_type": media_type.as_deref().unwrap_or("image/jpeg"),
                                        "data": data
                                    }
                                }));
                            }
                            ImageSource::Url => {
                                parts.push(serde_json::json!({
                                    "file_data": {
                                        "mime_type": media_type.as_deref().unwrap_or("image/jpeg"),
                                        "file_uri": data
                                    }
                                }));
                            }
                        },
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } => {
                            let response_val = serde_json::from_str::<serde_json::Value>(content)
                                .unwrap_or_else(|_| serde_json::json!({ "result": content }));
                            parts.push(serde_json::json!({
                                "functionResponse": {
                                    "name": tool_use_id,
                                    "response": response_val
                                }
                            }));
                        }
                        _ => {}
                    }
                }
                parts
            }
        }
    }

    fn convert_assistant_parts(&self, content: &MessageContent) -> Vec<serde_json::Value> {
        match content {
            MessageContent::Text(s) => vec![serde_json::json!({ "text": s })],
            MessageContent::Blocks(blocks) => {
                let mut parts = Vec::new();
                for block in blocks {
                    match block {
                        ContentBlock::Text { text } if !text.is_empty() => {
                            parts.push(serde_json::json!({ "text": text }));
                        }
                        ContentBlock::ToolUse { id: _, name, input } => {
                            // Extract thought_signature if present, pass clean args
                            let sig = input.get("__thought_signature").and_then(|v| v.as_str());
                            let mut clean_args = input.clone();
                            if let Some(obj) = clean_args.as_object_mut() {
                                obj.remove("__thought_signature");
                            }
                            let mut part = serde_json::json!({
                                "functionCall": {
                                    "name": name,
                                    "args": clean_args
                                }
                            });
                            if let Some(sig) = sig {
                                part["thoughtSignature"] = serde_json::json!(sig);
                            }
                            parts.push(part);
                        }
                        // Include thinking as thought parts for models that require it
                        ContentBlock::Thinking { text } if !text.is_empty() => {
                            parts.push(serde_json::json!({
                                "thought": true,
                                "text": text
                            }));
                        }
                        _ => {}
                    }
                }
                parts
            }
        }
    }

    /// Convert tool definitions to Gemini `tools` format
    fn convert_tools(&self, tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        if tools.is_empty() {
            return vec![];
        }

        let declarations: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                let mut decl = serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                });
                // Gemini expects "parameters" (OpenAPI schema), not "input_schema"
                if !t.input_schema.is_null() && t.input_schema != serde_json::json!({}) {
                    let mut schema = t.input_schema.clone();
                    // Strip keys Gemini doesn't support
                    strip_unsupported_schema_keys(&mut schema);
                    decl["parameters"] = schema;
                }
                decl
            })
            .collect();

        vec![serde_json::json!({ "functionDeclarations": declarations })]
    }

    fn map_finish_reason(reason: Option<&str>, has_function_call: bool) -> StopReason {
        if has_function_call {
            return StopReason::ToolUse;
        }
        match reason {
            Some("STOP") => StopReason::EndTurn,
            Some("MAX_TOKENS") => StopReason::MaxTokens,
            Some("SAFETY") => StopReason::Error,
            Some("RECITATION") => StopReason::Error,
            _ => StopReason::EndTurn,
        }
    }
}

/// Clean a JSON Schema for Gemini compatibility.
///
/// Gemini's function calling is strict about schema validation.
/// We remove all `required` arrays (Gemini rejects them if any entry doesn't
/// match a `properties` key, and some tool schemas are loose) plus unsupported
/// JSON Schema keywords. Tools validate their own inputs internally.
fn strip_unsupported_schema_keys(value: &mut serde_json::Value) {
    if let Some(obj) = value.as_object_mut() {
        obj.remove("$schema");
        obj.remove("additionalProperties");
        obj.remove("required");
        obj.remove("minItems");
        obj.remove("maxItems");
        obj.remove("minLength");
        obj.remove("maxLength");
        obj.remove("pattern");
        obj.remove("format");
        obj.remove("default");
        obj.remove("examples");
        obj.remove("title");

        for val in obj.values_mut() {
            strip_unsupported_schema_keys(val);
        }
    } else if let Some(arr) = value.as_array_mut() {
        for val in arr {
            strip_unsupported_schema_keys(val);
        }
    }
}

#[async_trait]
impl Provider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn supports_images(&self) -> bool {
        true // All Gemini models are natively multimodal
    }

    async fn stream(&self, params: StreamParams, tx: mpsc::Sender<StreamEvent>) -> Result<()> {
        let system_instruction = self.build_system_instruction(&params.system_prompt);
        let contents = self.convert_messages(&params.messages);
        let tools = self.convert_tools(&params.tools);

        let mut body = serde_json::json!({
            "systemInstruction": system_instruction,
            "contents": contents,
            "generationConfig": {
                "maxOutputTokens": self.max_tokens,
            }
        });

        if !tools.is_empty() {
            body["tools"] = serde_json::json!(tools);
        }

        let url = format!(
            "{}/models/{}:streamGenerateContent?alt=sse",
            GEMINI_API_BASE, self.model
        );

        debug!("Sending request to Gemini API (model: {})", self.model);

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .context("Failed to connect to Gemini API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error ({}): {}", status, error_body);
        }

        tx.send(StreamEvent::MessageStart).await.ok();

        let mut sse_parser = SseParser::new();
        let mut has_function_call = false;
        let mut finish_reason: Option<String> = None;
        let mut prompt_tokens: u64 = 0;
        let mut completion_tokens: u64 = 0;

        let mut stream = response.bytes_stream();
        use futures::StreamExt;

        let chunk_timeout = std::time::Duration::from_secs(120);
        while let Some(chunk) = tokio::time::timeout(chunk_timeout, stream.next())
            .await
            .context("Gemini stream timed out (no data for 60s)")?
        {
            let chunk = chunk.context("Error reading stream chunk")?;
            let text = String::from_utf8_lossy(&chunk);

            for sse_event in sse_parser.feed(&text) {
                match sse_event {
                    SseEvent::Done => break,
                    SseEvent::Data(data) => {
                        let response: GeminiStreamResponse = match serde_json::from_str(&data) {
                            Ok(r) => r,
                            Err(err) => {
                                warn!("Failed to parse Gemini SSE event: {err}");
                                continue;
                            }
                        };

                        // Check for API-level errors
                        if let Some(ref error) = response.error {
                            tx.send(StreamEvent::Error {
                                error: format!("{}: {}", error.code.unwrap_or(0), error.message),
                            })
                            .await
                            .ok();
                            continue;
                        }

                        // Process usage metadata
                        if let Some(ref usage) = response.usage_metadata {
                            prompt_tokens = usage.prompt_token_count.unwrap_or(0);
                            completion_tokens = usage.candidates_token_count.unwrap_or(0);
                        }

                        // Process candidates
                        if let Some(candidates) = response.candidates {
                            for candidate in &candidates {
                                if let Some(ref reason) = candidate.finish_reason {
                                    finish_reason = Some(reason.clone());
                                }

                                if let Some(ref content) = candidate.content {
                                    for part in &content.parts {
                                        if let Some(ref text) = part.text {
                                            if part.thought == Some(true) {
                                                tx.send(StreamEvent::ThinkingDelta {
                                                    text: text.clone(),
                                                })
                                                .await
                                                .ok();
                                            } else {
                                                tx.send(StreamEvent::TextDelta {
                                                    text: text.clone(),
                                                })
                                                .await
                                                .ok();
                                            }
                                        }

                                        if let Some(ref fc) = part.function_call {
                                            has_function_call = true;
                                            let id = format!("call_{}", uuid_v4_simple());
                                            let mut input =
                                                fc.args.clone().unwrap_or(serde_json::json!({}));

                                            // Store thought_signature in input so it survives
                                            // the message round-trip and can be sent back
                                            if let Some(ref sig) = part.thought_signature
                                                && let Some(obj) = input.as_object_mut()
                                            {
                                                obj.insert(
                                                    "__thought_signature".to_string(),
                                                    serde_json::json!(sig),
                                                );
                                            }

                                            tx.send(StreamEvent::ToolUseStart {
                                                id: id.clone(),
                                                name: fc.name.clone(),
                                            })
                                            .await
                                            .ok();

                                            tx.send(StreamEvent::ToolUseEnd {
                                                id,
                                                name: fc.name.clone(),
                                                input,
                                            })
                                            .await
                                            .ok();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Send usage and end
        tx.send(StreamEvent::Usage {
            input_tokens: prompt_tokens,
            output_tokens: completion_tokens,
            cache_creation_tokens: None,
            cache_read_tokens: None,
        })
        .await
        .ok();

        let stop_reason = Self::map_finish_reason(finish_reason.as_deref(), has_function_call);
        tx.send(StreamEvent::MessageEnd { stop_reason }).await.ok();

        Ok(())
    }
}

/// Generate a simple pseudo-UUID for tool call IDs
fn uuid_v4_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{t:032x}")
}

// ── Gemini streaming response types ──────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiStreamResponse {
    #[serde(default)]
    candidates: Option<Vec<GeminiCandidate>>,
    #[serde(default)]
    usage_metadata: Option<GeminiUsageMetadata>,
    #[serde(default)]
    error: Option<GeminiApiError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiCandidate {
    #[serde(default)]
    content: Option<GeminiContent>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiContent {
    #[serde(default)]
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiPart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thought: Option<bool>,
    #[serde(default)]
    function_call: Option<GeminiFunctionCall>,
    #[serde(default)]
    thought_signature: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiFunctionCall {
    name: String,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    #[serde(default)]
    prompt_token_count: Option<u64>,
    #[serde(default)]
    candidates_token_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct GeminiApiError {
    #[serde(default)]
    code: Option<u32>,
    #[serde(default)]
    message: String,
}

// ── Model listing ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GeminiModel {
    pub id: String,
    pub display_name: String,
}

/// Fetch available models from the Gemini API
pub async fn fetch_gemini_models(api_key: &str) -> Result<Vec<GeminiModel>> {
    let url = format!("{}/models", GEMINI_API_BASE);
    let client = Client::new();

    let resp = client
        .get(&url)
        .header("x-goog-api-key", api_key)
        .send()
        .await
        .context("Failed to connect to Gemini API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to fetch Gemini models: {status} {body}");
    }

    let page: GeminiModelsResponse = resp.json().await?;

    let models = page
        .models
        .into_iter()
        .filter(|m| {
            // Only include models that support generateContent
            m.supported_generation_methods
                .as_ref()
                .is_some_and(|methods| methods.iter().any(|method| method == "generateContent"))
        })
        .map(|m| {
            // Model name comes as "models/gemini-..." — strip the prefix
            let id = m
                .name
                .strip_prefix("models/")
                .unwrap_or(&m.name)
                .to_string();
            GeminiModel {
                id,
                display_name: m.display_name.unwrap_or_default(),
            }
        })
        .collect();

    Ok(models)
}

#[derive(Debug, Deserialize)]
struct GeminiModelsResponse {
    #[serde(default)]
    models: Vec<GeminiModelRaw>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiModelRaw {
    name: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    supported_generation_methods: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use lukan_core::models::messages::{ContentBlock, ImageSource, Message, MessageContent};
    use lukan_core::models::tools::ToolDefinition;
    use serde_json::json;

    fn make_provider() -> GeminiProvider {
        GeminiProvider::new("test-key".into(), "gemini-pro".into(), 4096)
    }

    // ── map_finish_reason ─────────────────────────────────────────────

    #[test]
    fn map_finish_reason_stop() {
        assert_eq!(
            GeminiProvider::map_finish_reason(Some("STOP"), false),
            StopReason::EndTurn
        );
    }

    #[test]
    fn map_finish_reason_max_tokens() {
        assert_eq!(
            GeminiProvider::map_finish_reason(Some("MAX_TOKENS"), false),
            StopReason::MaxTokens
        );
    }

    #[test]
    fn map_finish_reason_safety() {
        assert_eq!(
            GeminiProvider::map_finish_reason(Some("SAFETY"), false),
            StopReason::Error
        );
    }

    #[test]
    fn map_finish_reason_recitation() {
        assert_eq!(
            GeminiProvider::map_finish_reason(Some("RECITATION"), false),
            StopReason::Error
        );
    }

    #[test]
    fn map_finish_reason_function_call_overrides() {
        // When has_function_call is true, should always be ToolUse regardless of reason
        assert_eq!(
            GeminiProvider::map_finish_reason(Some("STOP"), true),
            StopReason::ToolUse
        );
        assert_eq!(
            GeminiProvider::map_finish_reason(None, true),
            StopReason::ToolUse
        );
    }

    #[test]
    fn map_finish_reason_none() {
        assert_eq!(
            GeminiProvider::map_finish_reason(None, false),
            StopReason::EndTurn
        );
    }

    // ── build_system_instruction ──────────────────────────────────────

    #[test]
    fn build_system_instruction_text() {
        let p = make_provider();
        let result = p.build_system_instruction(&SystemPrompt::Text("hello".into()));
        assert_eq!(result["parts"][0]["text"], "hello");
    }

    #[test]
    fn build_system_instruction_structured() {
        let p = make_provider();
        let result = p.build_system_instruction(&SystemPrompt::Structured {
            cached: vec!["a".into(), "b".into()],
            dynamic: "c".into(),
        });
        let parts = result["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0]["text"], "a");
        assert_eq!(parts[1]["text"], "b");
        assert_eq!(parts[2]["text"], "c");
    }

    #[test]
    fn build_system_instruction_structured_empty_dynamic() {
        let p = make_provider();
        let result = p.build_system_instruction(&SystemPrompt::Structured {
            cached: vec!["only".into()],
            dynamic: String::new(),
        });
        let parts = result["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
    }

    // ── convert_messages ──────────────────────────────────────────────

    #[test]
    fn convert_messages_user_text() {
        let p = make_provider();
        let result = p.convert_messages(&[Message::user("hi")]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "user");
        assert_eq!(result[0]["parts"][0]["text"], "hi");
    }

    #[test]
    fn convert_messages_assistant_text() {
        let p = make_provider();
        let result = p.convert_messages(&[Message::assistant("response")]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "model");
        assert_eq!(result[0]["parts"][0]["text"], "response");
    }

    #[test]
    fn convert_messages_tool_result() {
        let p = make_provider();
        let msgs = vec![Message {
            role: Role::Tool,
            content: MessageContent::Text("output".into()),
            tool_call_id: Some("my_tool".into()),
            name: None,
        }];
        let result = p.convert_messages(&msgs);
        assert_eq!(result[0]["role"], "user");
        let fr = &result[0]["parts"][0]["functionResponse"];
        assert_eq!(fr["name"], "my_tool");
    }

    #[test]
    fn convert_messages_merges_consecutive_same_role() {
        let p = make_provider();
        let msgs = vec![Message::user("first"), Message::user("second")];
        let result = p.convert_messages(&msgs);
        // Should be merged into one "user" entry
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["role"], "user");
        let parts = result[0]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["text"], "first");
        assert_eq!(parts[1]["text"], "second");
    }

    #[test]
    fn convert_messages_alternating_roles_not_merged() {
        let p = make_provider();
        let msgs = vec![
            Message::user("q"),
            Message::assistant("a"),
            Message::user("q2"),
        ];
        let result = p.convert_messages(&msgs);
        assert_eq!(result.len(), 3);
    }

    // ── convert_assistant_parts ───────────────────────────────────────

    #[test]
    fn convert_assistant_parts_tool_use() {
        let p = make_provider();
        let content = MessageContent::Blocks(vec![ContentBlock::ToolUse {
            id: "call_1".into(),
            name: "bash".into(),
            input: json!({"command": "ls"}),
        }]);
        let parts = p.convert_assistant_parts(&content);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["functionCall"]["name"], "bash");
        assert_eq!(parts[0]["functionCall"]["args"]["command"], "ls");
    }

    #[test]
    fn convert_assistant_parts_includes_thinking() {
        let p = make_provider();
        let content = MessageContent::Blocks(vec![ContentBlock::Thinking {
            text: "reasoning".into(),
        }]);
        let parts = p.convert_assistant_parts(&content);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(parts[0]["text"], "reasoning");
    }

    // ── convert_user_parts ────────────────────────────────────────────

    #[test]
    fn convert_user_parts_image_base64() {
        let p = make_provider();
        let content = MessageContent::Blocks(vec![ContentBlock::Image {
            source: ImageSource::Base64,
            data: "abc123".into(),
            media_type: Some("image/png".into()),
        }]);
        let parts = p.convert_user_parts(&content);
        assert_eq!(parts[0]["inline_data"]["mime_type"], "image/png");
        assert_eq!(parts[0]["inline_data"]["data"], "abc123");
    }

    #[test]
    fn convert_user_parts_image_url() {
        let p = make_provider();
        let content = MessageContent::Blocks(vec![ContentBlock::Image {
            source: ImageSource::Url,
            data: "https://example.com/img.png".into(),
            media_type: None,
        }]);
        let parts = p.convert_user_parts(&content);
        assert_eq!(
            parts[0]["file_data"]["file_uri"],
            "https://example.com/img.png"
        );
    }

    // ── convert_tools ─────────────────────────────────────────────────

    #[test]
    fn convert_tools_format() {
        let p = make_provider();
        let tools = vec![ToolDefinition {
            name: "bash".into(),
            description: "Run commands".into(),
            input_schema: json!({
                "type": "object",
                "properties": { "command": { "type": "string" } }
            }),
        }];
        let result = p.convert_tools(&tools);
        assert_eq!(result.len(), 1);
        let decls = result[0]["functionDeclarations"].as_array().unwrap();
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0]["name"], "bash");
        assert_eq!(decls[0]["description"], "Run commands");
        assert_eq!(
            decls[0]["parameters"]["properties"]["command"]["type"],
            "string"
        );
    }

    #[test]
    fn convert_tools_empty_returns_empty() {
        let p = make_provider();
        let result = p.convert_tools(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn convert_tools_null_schema_omits_parameters() {
        let p = make_provider();
        let tools = vec![ToolDefinition {
            name: "noop".into(),
            description: "No params".into(),
            input_schema: json!(null),
        }];
        let result = p.convert_tools(&tools);
        let decls = result[0]["functionDeclarations"].as_array().unwrap();
        assert!(decls[0].get("parameters").is_none());
    }

    #[test]
    fn convert_tools_empty_schema_omits_parameters() {
        let p = make_provider();
        let tools = vec![ToolDefinition {
            name: "noop".into(),
            description: "No params".into(),
            input_schema: json!({}),
        }];
        let result = p.convert_tools(&tools);
        let decls = result[0]["functionDeclarations"].as_array().unwrap();
        assert!(decls[0].get("parameters").is_none());
    }

    // ── strip_unsupported_schema_keys ─────────────────────────────────

    #[test]
    fn strip_unsupported_schema_keys_removes_known_keys() {
        let mut schema = json!({
            "type": "object",
            "$schema": "http://json-schema.org/draft-07/schema#",
            "additionalProperties": false,
            "required": ["name"],
            "properties": {
                "name": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": 100,
                    "pattern": "^[a-z]+$",
                    "format": "email",
                    "default": "foo",
                    "title": "Name",
                    "examples": ["bar"]
                }
            }
        });
        strip_unsupported_schema_keys(&mut schema);
        assert!(schema.get("$schema").is_none());
        assert!(schema.get("additionalProperties").is_none());
        assert!(schema.get("required").is_none());
        let name_prop = &schema["properties"]["name"];
        assert_eq!(name_prop["type"], "string");
        assert!(name_prop.get("minLength").is_none());
        assert!(name_prop.get("maxLength").is_none());
        assert!(name_prop.get("pattern").is_none());
        assert!(name_prop.get("format").is_none());
        assert!(name_prop.get("default").is_none());
        assert!(name_prop.get("title").is_none());
        assert!(name_prop.get("examples").is_none());
    }

    #[test]
    fn strip_unsupported_schema_keys_recurses_arrays() {
        let mut schema = json!({
            "type": "array",
            "items": { "type": "string", "minLength": 1 },
            "minItems": 1
        });
        strip_unsupported_schema_keys(&mut schema);
        assert!(schema.get("minItems").is_none());
        assert!(schema["items"].get("minLength").is_none());
        assert_eq!(schema["items"]["type"], "string");
    }

    // ── Gemini SSE type deserialization ───────────────────────────────

    #[test]
    fn deserialize_gemini_stream_response_text() {
        let data = r#"{"candidates":[{"content":{"parts":[{"text":"Hello"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":5}}"#;
        let resp: GeminiStreamResponse = serde_json::from_str(data).unwrap();
        let candidates = resp.candidates.unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].content.as_ref().unwrap().parts[0]
                .text
                .as_deref(),
            Some("Hello")
        );
        assert_eq!(candidates[0].finish_reason.as_deref(), Some("STOP"));
        let usage = resp.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, Some(10));
        assert_eq!(usage.candidates_token_count, Some(5));
    }

    #[test]
    fn deserialize_gemini_stream_response_function_call() {
        let data = r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"bash","args":{"command":"ls"}}}]}}]}"#;
        let resp: GeminiStreamResponse = serde_json::from_str(data).unwrap();
        let candidates = resp.candidates.unwrap();
        let part = &candidates[0].content.as_ref().unwrap().parts[0];
        let fc = part.function_call.as_ref().unwrap();
        assert_eq!(fc.name, "bash");
        assert_eq!(fc.args.as_ref().unwrap()["command"], "ls");
    }

    #[test]
    fn deserialize_gemini_stream_response_error() {
        let data = r#"{"error":{"code":429,"message":"Rate limited"}}"#;
        let resp: GeminiStreamResponse = serde_json::from_str(data).unwrap();
        let error = resp.error.unwrap();
        assert_eq!(error.code, Some(429));
        assert_eq!(error.message, "Rate limited");
    }

    #[test]
    fn deserialize_gemini_stream_response_empty() {
        let data = r#"{}"#;
        let resp: GeminiStreamResponse = serde_json::from_str(data).unwrap();
        assert!(resp.candidates.is_none());
        assert!(resp.usage_metadata.is_none());
        assert!(resp.error.is_none());
    }
}
