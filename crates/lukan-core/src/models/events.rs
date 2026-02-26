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

/// A task in a submitted plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanTask {
    pub title: String,
    pub detail: String,
}

/// A question item for the PlannerQuestion tool
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerQuestionItem {
    pub header: String,
    pub question: String,
    pub options: Vec<PlannerQuestionOption>,
    #[serde(default)]
    pub multi_select: bool,
}

/// An option in a PlannerQuestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerQuestionOption {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Response from the UI to a plan review request
#[derive(Debug, Clone)]
pub enum PlanReviewResponse {
    /// User accepted the plan (optionally with modified task list)
    Accepted {
        modified_tasks: Option<Vec<PlanTask>>,
    },
    /// User rejected the plan with feedback
    Rejected { feedback: String },
    /// User wants changes to a specific task
    TaskFeedback { task_index: usize, feedback: String },
}

/// Streaming events emitted by providers and the agent loop
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
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

    /// Plan submitted for user review
    PlanReview {
        id: String,
        title: String,
        plan: String,
        tasks: Vec<PlanTask>,
    },

    /// Planner asking clarifying questions
    PlannerQuestion {
        id: String,
        questions: Vec<PlannerQuestionItem>,
    },

    /// Permission mode changed (e.g. planner → auto after plan accept)
    ModeChanged { mode: String },

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

    /// Progress update from a running Explore sub-agent
    ExploreProgress {
        id: String,
        task: String,
        tool_calls: u32,
        tokens: u64,
        elapsed_secs: u64,
        activity: String,
    },

    /// System event from a plugin (fire-and-forget notification)
    SystemNotification {
        source: String,
        level: String,
        detail: String,
    },
}

/// A tool call that needs user approval
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequest {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// Response from the UI to an approval request (internal, not serialized over the wire)
#[derive(Debug, Clone)]
pub enum ApprovalResponse {
    /// User approved specific tools (by ID)
    Approved { approved_ids: Vec<String> },
    /// User denied all pending tools
    DeniedAll,
    /// Approve + persist pattern to config allow list
    AlwaysAllow {
        approved_ids: Vec<String>,
        tools: Vec<ToolApprovalRequest>,
    },
}
