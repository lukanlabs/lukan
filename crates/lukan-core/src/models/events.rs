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

/// Lightweight task info for streaming to the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskInfo {
    pub id: u32,
    pub title: String,
    pub status: String,
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
        /// File content after the edit (for FileViewer inline preview)
        #[serde(skip_serializing_if = "Option::is_none")]
        after_content: Option<String>,
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

    /// A queued user message was injected mid-turn
    QueuedMessageInjected { text: String },

    /// Updated task list (emitted after plan acceptance or task tool calls)
    TasksUpdate { tasks: Vec<TaskInfo> },

    /// Sub-agent status update (forwarded from daemon to TUI)
    SubAgentUpdate {
        id: String,
        task: String,
        status: String,
        turns: u32,
        input_tokens: u64,
        output_tokens: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<String>,
        /// Chat messages for the spectator view
        chat_messages: Vec<SubAgentChatMessage>,
    },
}

/// A chat message from a sub-agent conversation (for spectator view)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentChatMessage {
    pub role: String,
    pub content: String,
}

/// A tool call that needs user approval
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequest {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_hint: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── StopReason ──────────────────────────────────────────────────

    #[test]
    fn test_stop_reason_serde() {
        assert_eq!(
            serde_json::to_string(&StopReason::EndTurn).unwrap(),
            r#""end_turn""#
        );
        assert_eq!(
            serde_json::to_string(&StopReason::ToolUse).unwrap(),
            r#""tool_use""#
        );
        assert_eq!(
            serde_json::to_string(&StopReason::MaxTokens).unwrap(),
            r#""max_tokens""#
        );
        assert_eq!(
            serde_json::to_string(&StopReason::Error).unwrap(),
            r#""error""#
        );

        let parsed: StopReason = serde_json::from_str(r#""end_turn""#).unwrap();
        assert_eq!(parsed, StopReason::EndTurn);
    }

    // ── PlanTask ────────────────────────────────────────────────────

    #[test]
    fn test_plan_task_serde() {
        let task = PlanTask {
            title: "Refactor module".into(),
            detail: "Split into smaller files".into(),
        };
        let json = serde_json::to_string(&task).unwrap();
        let parsed: PlanTask = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.title, "Refactor module");
        assert_eq!(parsed.detail, "Split into smaller files");
    }

    // ── TaskInfo ────────────────────────────────────────────────────

    #[test]
    fn test_task_info_serde() {
        let info = TaskInfo {
            id: 1,
            title: "Build".into(),
            status: "running".into(),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: TaskInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, 1);
        assert_eq!(parsed.status, "running");
    }

    // ── PlannerQuestionItem / PlannerQuestionOption ──────────────────

    #[test]
    fn test_planner_question_serde() {
        let item = PlannerQuestionItem {
            header: "Architecture".into(),
            question: "Which pattern?".into(),
            options: vec![
                PlannerQuestionOption {
                    label: "MVC".into(),
                    description: Some("Model-View-Controller".into()),
                },
                PlannerQuestionOption {
                    label: "Hexagonal".into(),
                    description: None,
                },
            ],
            multi_select: true,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains(r#""multiSelect":true"#));

        let parsed: PlannerQuestionItem = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.options.len(), 2);
        assert!(parsed.multi_select);
        assert_eq!(parsed.options[0].label, "MVC");
        assert!(parsed.options[1].description.is_none());
    }

    #[test]
    fn test_planner_question_multi_select_default() {
        let json = r#"{"header":"h","question":"q","options":[]}"#;
        let parsed: PlannerQuestionItem = serde_json::from_str(json).unwrap();
        assert!(!parsed.multi_select);
    }

    // ── StreamEvent ─────────────────────────────────────────────────

    #[test]
    fn test_stream_event_text_delta_serde() {
        let event = StreamEvent::TextDelta {
            text: "Hello".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"text_delta""#));
        assert!(json.contains(r#""text":"Hello""#));

        let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            StreamEvent::TextDelta { text } => assert_eq!(text, "Hello"),
            _ => panic!("Expected TextDelta"),
        }
    }

    #[test]
    fn test_stream_event_message_start_serde() {
        let event = StreamEvent::MessageStart;
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"message_start""#));
    }

    #[test]
    fn test_stream_event_tool_use_start_serde() {
        let event = StreamEvent::ToolUseStart {
            id: "t1".into(),
            name: "Bash".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"tool_use_start""#));

        let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            StreamEvent::ToolUseStart { id, name } => {
                assert_eq!(id, "t1");
                assert_eq!(name, "Bash");
            }
            _ => panic!("Expected ToolUseStart"),
        }
    }

    #[test]
    fn test_stream_event_usage_serde() {
        let event = StreamEvent::Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_tokens: Some(10),
            cache_read_tokens: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""inputTokens":100"#));
        assert!(json.contains(r#""outputTokens":50"#));
        // cache_read_tokens is None, should be skipped
        assert!(!json.contains("cacheReadTokens"));

        let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
        match parsed {
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            } => {
                assert_eq!(input_tokens, 100);
                assert_eq!(output_tokens, 50);
                assert_eq!(cache_creation_tokens, Some(10));
                assert!(cache_read_tokens.is_none());
            }
            _ => panic!("Expected Usage"),
        }
    }

    #[test]
    fn test_stream_event_message_end_serde() {
        let event = StreamEvent::MessageEnd {
            stop_reason: StopReason::EndTurn,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""stopReason":"end_turn""#));
    }

    #[test]
    fn test_stream_event_error_serde() {
        let event = StreamEvent::Error {
            error: "timeout".into(),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#""error":"timeout""#));
    }

    #[test]
    fn test_stream_event_tool_result_serde() {
        let event = StreamEvent::ToolResult {
            id: "t1".into(),
            name: "Bash".into(),
            content: "output".into(),
            is_error: Some(false),
            diff: None,
            image: None,
            after_content: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""type":"tool_result""#));
        // diff and image should be omitted
        assert!(!json.contains("diff"));
        assert!(!json.contains("image"));
    }

    // ── ToolApprovalRequest ─────────────────────────────────────────

    #[test]
    fn test_tool_approval_request_serde() {
        let req = ToolApprovalRequest {
            id: "tu-1".into(),
            name: "Bash".into(),
            input: serde_json::json!({"command": "rm -rf /"}),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""name":"Bash""#));

        let parsed: ToolApprovalRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "tu-1");
        assert_eq!(parsed.input["command"], "rm -rf /");
    }
}
