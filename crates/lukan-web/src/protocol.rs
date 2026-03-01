use serde::{Deserialize, Serialize};

use lukan_core::models::checkpoints::Checkpoint;
use lukan_core::models::events::{PlanTask, PlannerQuestionItem};
use lukan_core::models::messages::Message;
use lukan_core::models::sessions::SessionSummary;
use lukan_core::workers::{
    WorkerCreateInput, WorkerDetail, WorkerRun, WorkerSummary, WorkerUpdateInput,
};

/// Messages sent from the client (browser) to the server
#[derive(Debug, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[allow(dead_code)] // Stub variants have unread fields
pub enum ClientMessage {
    // Chat
    SendMessage {
        content: String,
    },
    Approve {
        approved_ids: Vec<String>,
    },
    AlwaysAllow {
        approved_ids: Vec<String>,
        tools: Vec<lukan_core::models::events::ToolApprovalRequest>,
    },
    DenyAll,
    AnswerQuestion {
        answer: String,
    },
    Abort,

    // Sessions
    LoadSession {
        session_id: String,
    },
    NewSession {
        name: Option<String>,
    },
    ListSessions,
    DeleteSession {
        session_id: String,
    },

    // Model
    ListModels,
    SetModel {
        model: String,
    },

    // Config
    GetConfig,
    SetConfig {
        config: serde_json::Value,
    },
    SetPermissionMode {
        mode: String,
    },

    // Auth
    Auth {
        token: String,
    },
    AuthLogin {
        password: String,
    },

    // Stubs (not implemented yet)
    GetSubAgentDetail {
        id: String,
    },
    AbortSubAgent {
        id: String,
    },
    SetScreenshots {
        enabled: bool,
    },
    ListWorkers,
    CreateWorker {
        worker: WorkerCreateInput,
    },
    UpdateWorker {
        id: String,
        patch: WorkerUpdateInput,
    },
    DeleteWorker {
        id: String,
    },
    ToggleWorker {
        id: String,
        enabled: bool,
    },
    GetWorkerDetail {
        id: String,
    },
    GetWorkerRunDetail {
        worker_id: String,
        run_id: String,
    },
    PlanAccept {
        tasks: Option<serde_json::Value>,
    },
    PlanReject {
        feedback: String,
    },
    PlanTaskFeedback {
        task_index: u32,
        feedback: String,
    },

    // Terminal
    TerminalCreate {
        cwd: Option<String>,
        cols: u16,
        rows: u16,
    },
    TerminalInput {
        session_id: String,
        data: String,
    },
    TerminalResize {
        session_id: String,
        cols: u16,
        rows: u16,
    },
    TerminalDestroy {
        session_id: String,
    },
    TerminalList,
}

/// Messages sent from the server to the client (browser)
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[allow(dead_code)]
pub enum ServerMessage {
    Init {
        session_id: String,
        messages: Vec<Message>,
        checkpoints: Vec<Checkpoint>,
        token_usage: TokenUsage,
        context_size: u64,
        permission_mode: String,
        provider_name: String,
        model_name: String,
        browser_screenshots: bool,
    },
    ProcessingComplete {
        session_id: String,
        messages: Vec<Message>,
        checkpoints: Vec<Checkpoint>,
        #[serde(skip_serializing_if = "Option::is_none")]
        context_size: Option<u64>,
    },
    SessionList {
        sessions: Vec<SessionSummary>,
    },
    SessionLoaded {
        session_id: String,
        messages: Vec<Message>,
        checkpoints: Vec<Checkpoint>,
        token_usage: TokenUsage,
        context_size: u64,
    },
    ModelList {
        models: Vec<String>,
        current: String,
    },
    ModelChanged {
        provider_name: String,
        model_name: String,
    },
    ConfigValues {
        config: serde_json::Value,
    },
    ConfigSaved {
        config: serde_json::Value,
    },
    SubAgentsUpdate {
        agents: Vec<serde_json::Value>,
    },
    WorkersUpdate {
        workers: Vec<WorkerSummary>,
    },
    WorkerDetail {
        worker: WorkerDetail,
    },
    WorkerRunDetail {
        run: WorkerRun,
    },
    WorkerNotification {
        worker_id: String,
        worker_name: String,
        status: String,
        summary: String,
    },
    AuthRequired,
    AuthOk {
        token: String,
    },
    AuthError {
        error: String,
    },
    Error {
        error: String,
    },
    ModeChanged {
        mode: String,
    },
    ScreenshotsChanged {
        enabled: bool,
    },
    PlanReview {
        id: String,
        title: String,
        plan: String,
        tasks: Vec<PlanTask>,
    },
    PlannerQuestion {
        id: String,
        questions: Vec<PlannerQuestionItem>,
    },

    // Terminal
    TerminalCreated {
        id: String,
        cols: u16,
        rows: u16,
    },
    TerminalSessions {
        sessions: Vec<TerminalSessionInfoDto>,
    },
    TerminalOutput {
        session_id: String,
        data: String,
    },
    TerminalExited {
        session_id: String,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSessionInfoDto {
    pub id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_message_init_camel_case_fields() {
        let msg = ServerMessage::Init {
            session_id: "abc123".into(),
            messages: vec![],
            checkpoints: vec![],
            token_usage: TokenUsage {
                input: 100,
                output: 50,
                cache_creation: None,
                cache_read: Some(10),
            },
            context_size: 200000,
            permission_mode: "auto".into(),
            provider_name: "anthropic".into(),
            model_name: "claude-sonnet".into(),
            browser_screenshots: false,
        };
        let json = serde_json::to_string(&msg).unwrap();
        // Variant tag should be snake_case
        assert!(
            json.contains(r#""type":"init""#),
            "tag should be snake_case: {json}"
        );
        // Fields should be camelCase
        assert!(json.contains(r#""sessionId""#), "sessionId field: {json}");
        assert!(json.contains(r#""tokenUsage""#), "tokenUsage field: {json}");
        assert!(
            json.contains(r#""contextSize""#),
            "contextSize field: {json}"
        );
        assert!(
            json.contains(r#""permissionMode""#),
            "permissionMode field: {json}"
        );
        assert!(
            json.contains(r#""providerName""#),
            "providerName field: {json}"
        );
        assert!(json.contains(r#""modelName""#), "modelName field: {json}");
        assert!(
            json.contains(r#""browserScreenshots""#),
            "browserScreenshots field: {json}"
        );
        // TokenUsage inner fields should also be camelCase
        assert!(json.contains(r#""cacheRead""#), "cacheRead field: {json}");
        // Should NOT contain snake_case field names
        assert!(
            !json.contains("session_id"),
            "should not have snake_case session_id: {json}"
        );
        assert!(
            !json.contains("token_usage"),
            "should not have snake_case token_usage: {json}"
        );
        assert!(
            !json.contains("context_size"),
            "should not have snake_case context_size: {json}"
        );
    }

    #[test]
    fn test_client_message_deserialize_camel_case() {
        let json = r#"{"type":"load_session","sessionId":"abc123"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::LoadSession { session_id } => {
                assert_eq!(session_id, "abc123");
            }
            _ => panic!("Expected LoadSession variant"),
        }
    }

    #[test]
    fn test_stream_event_camel_case_fields() {
        use lukan_core::models::events::StreamEvent;
        let event = StreamEvent::Usage {
            input_tokens: 1000,
            output_tokens: 500,
            cache_creation_tokens: None,
            cache_read_tokens: Some(200),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""inputTokens""#), "inputTokens: {json}");
        assert!(json.contains(r#""outputTokens""#), "outputTokens: {json}");
        assert!(
            json.contains(r#""cacheReadTokens""#),
            "cacheReadTokens: {json}"
        );
        assert!(
            !json.contains("input_tokens"),
            "should not have snake_case: {json}"
        );

        let event2 = StreamEvent::MessageEnd {
            stop_reason: lukan_core::models::events::StopReason::EndTurn,
        };
        let json2 = serde_json::to_string(&event2).unwrap();
        assert!(json2.contains(r#""stopReason""#), "stopReason: {json2}");
    }

    #[test]
    fn test_content_block_camel_case_fields() {
        use lukan_core::models::messages::ContentBlock;
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu_123".into(),
            content: "result".into(),
            is_error: Some(true),
            diff: None,
            image: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""toolUseId""#), "toolUseId: {json}");
        assert!(json.contains(r#""isError""#), "isError: {json}");
        assert!(
            !json.contains("is_error"),
            "should not have snake_case is_error: {json}"
        );
    }

    #[test]
    fn test_worker_detail_serialization() {
        use lukan_core::workers::{
            WorkerDefinition, WorkerDetail, WorkerRun, WorkerSummary, WorkerTokenUsage,
        };
        let detail = WorkerDetail {
            summary: WorkerSummary {
                definition: WorkerDefinition {
                    id: "abc123".into(),
                    name: "prueba".into(),
                    schedule: "every:10s".into(),
                    prompt: "test".into(),
                    tools: None,
                    provider: None,
                    model: None,
                    enabled: true,
                    notify: None,
                    created_at: "2024-01-01".into(),
                    last_run_at: None,
                    last_run_status: Some("success".into()),
                },
                recent_run_status: Some("success".into()),
            },
            recent_runs: vec![WorkerRun {
                id: "run1".into(),
                worker_id: "abc123".into(),
                started_at: "2024-01-01T00:00:00Z".into(),
                completed_at: Some("2024-01-01T00:01:00Z".into()),
                status: "success".into(),
                output: "done".into(),
                error: None,
                token_usage: WorkerTokenUsage::default(),
                turns: 3,
            }],
        };
        let msg = ServerMessage::WorkerDetail { worker: detail };
        let json = serde_json::to_string_pretty(&msg).unwrap();
        eprintln!("WorkerDetail JSON:\n{json}");
        assert!(
            json.contains(r#""type": "worker_detail""#),
            "tag should be worker_detail: {json}"
        );
        assert!(
            json.contains(r#""worker""#),
            "should have worker field: {json}"
        );
        assert!(
            json.contains(r#""recentRuns""#),
            "should have recentRuns: {json}"
        );
    }
}
