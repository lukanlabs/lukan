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
                        ContentBlock::Text { text } => {
                            if !text.is_empty() {
                                parts.push(serde_json::json!({ "text": text }));
                            }
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
                        ContentBlock::Text { text } => {
                            if !text.is_empty() {
                                parts.push(serde_json::json!({ "text": text }));
                            }
                        }
                        ContentBlock::ToolUse { id: _, name, input } => {
                            parts.push(serde_json::json!({
                                "functionCall": {
                                    "name": name,
                                    "args": input
                                }
                            }));
                        }
                        ContentBlock::Thinking { .. } => {
                            // Skip thinking blocks
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
            "{}/models/{}:streamGenerateContent?alt=sse&key={}",
            GEMINI_API_BASE, self.model, self.api_key
        );

        debug!("Sending request to Gemini API (model: {})", self.model);

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
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

        let chunk_timeout = std::time::Duration::from_secs(60);
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
                                            tx.send(StreamEvent::TextDelta { text: text.clone() })
                                                .await
                                                .ok();
                                        }

                                        if let Some(ref fc) = part.function_call {
                                            has_function_call = true;
                                            let id = format!("call_{}", uuid_v4_simple());
                                            let input =
                                                fc.args.clone().unwrap_or(serde_json::json!({}));

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
    function_call: Option<GeminiFunctionCall>,
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
    let url = format!("{}/models?key={}", GEMINI_API_BASE, api_key);
    let client = Client::new();

    let resp = client
        .get(&url)
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
