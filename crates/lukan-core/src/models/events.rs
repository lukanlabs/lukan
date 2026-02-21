use serde::{Deserialize, Serialize};

/// Stop reason from the LLM
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Error,
}

/// Streaming events emitted by providers and the agent loop
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Start of a new message
    MessageStart,

    /// Incremental text content
    TextDelta { text: String },

    /// Incremental thinking/reasoning content
    ThinkingDelta { text: String },

    /// A tool call has started
    ToolUseStart { id: String, name: String },

    /// Incremental tool input JSON
    ToolUseDelta { input: String },

    /// Tool call complete with parsed input
    ToolUseEnd {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Progress update from a running tool
    ToolProgress {
        id: String,
        name: String,
        content: String,
    },

    /// Result from a completed tool execution
    ToolResult {
        id: String,
        name: String,
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        image: Option<String>,
    },

    /// Tool calls require user approval
    ApprovalRequired { tools: Vec<ToolApprovalRequest> },

    /// Token usage information
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_creation_tokens: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_read_tokens: Option<u64>,
    },

    /// End of message with stop reason
    MessageEnd { stop_reason: StopReason },

    /// An error occurred
    Error { error: String },
}

/// A tool call that needs user approval
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequest {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}
