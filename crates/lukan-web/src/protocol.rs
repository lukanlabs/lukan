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
    // Chat (per-session: sessionId in JSON = tab/agent instance ID)
    SendMessage {
        content: String,
        #[serde(default)]
        session_id: Option<String>,
    },
    Approve {
        approved_ids: Vec<String>,
        #[serde(default)]
        session_id: Option<String>,
    },
    AlwaysAllow {
        approved_ids: Vec<String>,
        tools: Vec<lukan_core::models::events::ToolApprovalRequest>,
        #[serde(default)]
        session_id: Option<String>,
    },
    DenyAll {
        #[serde(default)]
        session_id: Option<String>,
    },
    AnswerQuestion {
        answer: String,
        #[serde(default)]
        session_id: Option<String>,
    },
    Abort {
        #[serde(default)]
        session_id: Option<String>,
    },

    // Sessions
    LoadSession {
        #[serde(default)]
        session_id: Option<String>,
        /// Saved session ID to load (new protocol); falls back to session_id if absent
        #[serde(default)]
        id: Option<String>,
    },
    NewSession {
        name: Option<String>,
        #[serde(default)]
        session_id: Option<String>,
    },
    ListSessions,
    DeleteSession {
        session_id: String,
    },

    // Agent tabs (multi-agent)
    CreateAgentTab,
    DestroyAgentTab {
        session_id: String,
    },
    RenameAgentTab {
        session_id: String,
        label: String,
    },
    SendToBackground {
        #[serde(default)]
        session_id: Option<String>,
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
        #[serde(default)]
        session_id: Option<String>,
    },
    PlanReject {
        feedback: String,
        #[serde(default)]
        session_id: Option<String>,
    },
    PlanTaskFeedback {
        task_index: u32,
        feedback: String,
        #[serde(default)]
        session_id: Option<String>,
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
    TerminalReconnect {
        session_id: String,
    },
    TerminalRename {
        session_id: String,
        name: String,
    },
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
        /// Agent tab ID for routing (new multi-tab protocol)
        #[serde(skip_serializing_if = "Option::is_none")]
        tab_id: Option<String>,
        /// True when the turn was cancelled by the user (abort)
        #[serde(skip_serializing_if = "Option::is_none")]
        aborted: Option<bool>,
    },
    AgentTabCreated {
        session_id: String,
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
        #[serde(skip_serializing_if = "Option::is_none")]
        scrollback: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
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
        // Old protocol: sessionId = saved session to load
        let json = r#"{"type":"load_session","sessionId":"abc123"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::LoadSession { session_id, id } => {
                assert_eq!(session_id, Some("abc123".to_string()));
                assert_eq!(id, None);
            }
            _ => panic!("Expected LoadSession variant"),
        }

        // New protocol: sessionId = tab ID, id = saved session
        let json2 = r#"{"type":"load_session","sessionId":"tab-1","id":"saved-abc"}"#;
        let msg2: ClientMessage = serde_json::from_str(json2).unwrap();
        match msg2 {
            ClientMessage::LoadSession { session_id, id } => {
                assert_eq!(session_id, Some("tab-1".to_string()));
                assert_eq!(id, Some("saved-abc".to_string()));
            }
            _ => panic!("Expected LoadSession variant"),
        }

        // DenyAll with optional session_id
        let json3 = r#"{"type":"deny_all"}"#;
        let msg3: ClientMessage = serde_json::from_str(json3).unwrap();
        match msg3 {
            ClientMessage::DenyAll { session_id } => {
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected DenyAll variant"),
        }

        // CreateAgentTab
        let json4 = r#"{"type":"create_agent_tab"}"#;
        let _msg4: ClientMessage = serde_json::from_str(json4).unwrap();
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

    // --- TokenUsage serialization/deserialization ---

    #[test]
    fn test_token_usage_roundtrip() {
        let usage = TokenUsage {
            input: 1000,
            output: 500,
            cache_creation: Some(200),
            cache_read: Some(100),
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(json.contains(r#""cacheCreation""#), "cacheCreation: {json}");
        assert!(json.contains(r#""cacheRead""#), "cacheRead: {json}");
        assert!(
            !json.contains("cache_creation"),
            "no snake_case cache_creation: {json}"
        );
        assert!(
            !json.contains("cache_read"),
            "no snake_case cache_read: {json}"
        );

        let parsed: TokenUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.input, 1000);
        assert_eq!(parsed.output, 500);
        assert_eq!(parsed.cache_creation, Some(200));
        assert_eq!(parsed.cache_read, Some(100));
    }

    #[test]
    fn test_token_usage_skip_none_fields() {
        let usage = TokenUsage {
            input: 42,
            output: 7,
            cache_creation: None,
            cache_read: None,
        };
        let json = serde_json::to_string(&usage).unwrap();
        assert!(
            !json.contains("cacheCreation"),
            "None cacheCreation should be skipped: {json}"
        );
        assert!(
            !json.contains("cacheRead"),
            "None cacheRead should be skipped: {json}"
        );
    }

    #[test]
    fn test_token_usage_deserialize_missing_optional_fields() {
        let json = r#"{"input":10,"output":5}"#;
        let parsed: TokenUsage = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.input, 10);
        assert_eq!(parsed.output, 5);
        assert_eq!(parsed.cache_creation, None);
        assert_eq!(parsed.cache_read, None);
    }

    // --- ClientMessage deserialization (additional variants) ---

    #[test]
    fn test_client_message_send_message() {
        let json = r#"{"type":"send_message","content":"hello world"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::SendMessage {
                content,
                session_id,
            } => {
                assert_eq!(content, "hello world");
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected SendMessage"),
        }
    }

    #[test]
    fn test_client_message_send_message_with_session() {
        let json = r#"{"type":"send_message","content":"hello","sessionId":"tab-42"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::SendMessage {
                content,
                session_id,
            } => {
                assert_eq!(content, "hello");
                assert_eq!(session_id, Some("tab-42".to_string()));
            }
            _ => panic!("Expected SendMessage"),
        }
    }

    #[test]
    fn test_client_message_approve() {
        let json = r#"{"type":"approve","approvedIds":["id1","id2"]}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Approve {
                approved_ids,
                session_id,
            } => {
                assert_eq!(approved_ids, vec!["id1", "id2"]);
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected Approve"),
        }
    }

    #[test]
    fn test_client_message_abort() {
        let json = r#"{"type":"abort","sessionId":"sess-1"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Abort { session_id } => {
                assert_eq!(session_id, Some("sess-1".to_string()));
            }
            _ => panic!("Expected Abort"),
        }
    }

    #[test]
    fn test_client_message_answer_question() {
        let json = r#"{"type":"answer_question","answer":"yes"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::AnswerQuestion { answer, session_id } => {
                assert_eq!(answer, "yes");
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected AnswerQuestion"),
        }
    }

    #[test]
    fn test_client_message_new_session() {
        let json = r#"{"type":"new_session","name":"My Session"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::NewSession { name, session_id } => {
                assert_eq!(name, Some("My Session".to_string()));
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected NewSession"),
        }
    }

    #[test]
    fn test_client_message_list_sessions() {
        let json = r#"{"type":"list_sessions"}"#;
        let _msg: ClientMessage = serde_json::from_str(json).unwrap();
    }

    #[test]
    fn test_client_message_delete_session() {
        let json = r#"{"type":"delete_session","sessionId":"sess-x"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::DeleteSession { session_id } => {
                assert_eq!(session_id, "sess-x");
            }
            _ => panic!("Expected DeleteSession"),
        }
    }

    #[test]
    fn test_client_message_set_model() {
        let json = r#"{"type":"set_model","model":"claude-opus-4-20250514"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::SetModel { model } => {
                assert_eq!(model, "claude-opus-4-20250514");
            }
            _ => panic!("Expected SetModel"),
        }
    }

    #[test]
    fn test_client_message_list_models() {
        let json = r#"{"type":"list_models"}"#;
        let _msg: ClientMessage = serde_json::from_str(json).unwrap();
    }

    #[test]
    fn test_client_message_auth() {
        let json = r#"{"type":"auth","token":"abc123"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Auth { token } => {
                assert_eq!(token, "abc123");
            }
            _ => panic!("Expected Auth"),
        }
    }

    #[test]
    fn test_client_message_auth_login() {
        let json = r#"{"type":"auth_login","password":"secret"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::AuthLogin { password } => {
                assert_eq!(password, "secret");
            }
            _ => panic!("Expected AuthLogin"),
        }
    }

    #[test]
    fn test_client_message_set_config() {
        let json = r#"{"type":"set_config","config":{"key":"value"}}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::SetConfig { config } => {
                assert_eq!(config["key"], "value");
            }
            _ => panic!("Expected SetConfig"),
        }
    }

    #[test]
    fn test_client_message_get_config() {
        let json = r#"{"type":"get_config"}"#;
        let _msg: ClientMessage = serde_json::from_str(json).unwrap();
    }

    #[test]
    fn test_client_message_set_permission_mode() {
        let json = r#"{"type":"set_permission_mode","mode":"manual"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::SetPermissionMode { mode } => {
                assert_eq!(mode, "manual");
            }
            _ => panic!("Expected SetPermissionMode"),
        }
    }

    #[test]
    fn test_client_message_destroy_agent_tab() {
        let json = r#"{"type":"destroy_agent_tab","sessionId":"tab-1"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::DestroyAgentTab { session_id } => {
                assert_eq!(session_id, "tab-1");
            }
            _ => panic!("Expected DestroyAgentTab"),
        }
    }

    #[test]
    fn test_client_message_rename_agent_tab() {
        let json = r#"{"type":"rename_agent_tab","sessionId":"tab-1","label":"Research"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::RenameAgentTab { session_id, label } => {
                assert_eq!(session_id, "tab-1");
                assert_eq!(label, "Research");
            }
            _ => panic!("Expected RenameAgentTab"),
        }
    }

    #[test]
    fn test_client_message_terminal_create() {
        let json = r#"{"type":"terminal_create","cwd":"/tmp","cols":80,"rows":24}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::TerminalCreate { cwd, cols, rows } => {
                assert_eq!(cwd, Some("/tmp".to_string()));
                assert_eq!(cols, 80);
                assert_eq!(rows, 24);
            }
            _ => panic!("Expected TerminalCreate"),
        }
    }

    #[test]
    fn test_client_message_terminal_input() {
        let json = r#"{"type":"terminal_input","sessionId":"t1","data":"bHM="}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::TerminalInput { session_id, data } => {
                assert_eq!(session_id, "t1");
                assert_eq!(data, "bHM=");
            }
            _ => panic!("Expected TerminalInput"),
        }
    }

    #[test]
    fn test_client_message_terminal_resize() {
        let json = r#"{"type":"terminal_resize","sessionId":"t1","cols":120,"rows":40}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::TerminalResize {
                session_id,
                cols,
                rows,
            } => {
                assert_eq!(session_id, "t1");
                assert_eq!(cols, 120);
                assert_eq!(rows, 40);
            }
            _ => panic!("Expected TerminalResize"),
        }
    }

    #[test]
    fn test_client_message_terminal_destroy() {
        let json = r#"{"type":"terminal_destroy","sessionId":"t1"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::TerminalDestroy { session_id } => {
                assert_eq!(session_id, "t1");
            }
            _ => panic!("Expected TerminalDestroy"),
        }
    }

    #[test]
    fn test_client_message_terminal_list() {
        let json = r#"{"type":"terminal_list"}"#;
        let _msg: ClientMessage = serde_json::from_str(json).unwrap();
    }

    #[test]
    fn test_client_message_plan_accept() {
        let json = r#"{"type":"plan_accept","tasks":[{"name":"t1"}],"sessionId":"s1"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::PlanAccept { tasks, session_id } => {
                assert!(tasks.is_some());
                assert_eq!(session_id, Some("s1".to_string()));
            }
            _ => panic!("Expected PlanAccept"),
        }
    }

    #[test]
    fn test_client_message_plan_reject() {
        let json = r#"{"type":"plan_reject","feedback":"not good enough"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::PlanReject {
                feedback,
                session_id,
            } => {
                assert_eq!(feedback, "not good enough");
                assert_eq!(session_id, None);
            }
            _ => panic!("Expected PlanReject"),
        }
    }

    #[test]
    fn test_client_message_set_screenshots() {
        let json = r#"{"type":"set_screenshots","enabled":true}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::SetScreenshots { enabled } => {
                assert!(enabled);
            }
            _ => panic!("Expected SetScreenshots"),
        }
    }

    #[test]
    fn test_client_message_invalid_type_fails() {
        let json = r#"{"type":"nonexistent_variant"}"#;
        let result = serde_json::from_str::<ClientMessage>(json);
        assert!(result.is_err(), "Unknown type should fail to deserialize");
    }

    #[test]
    fn test_client_message_missing_required_field_fails() {
        // send_message requires "content"
        let json = r#"{"type":"send_message"}"#;
        let result = serde_json::from_str::<ClientMessage>(json);
        assert!(
            result.is_err(),
            "Missing required field should fail: {result:?}"
        );
    }

    // --- ServerMessage serialization ---

    #[test]
    fn test_server_message_error() {
        let msg = ServerMessage::Error {
            error: "something broke".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"error""#), "type tag: {json}");
        assert!(
            json.contains(r#""error":"something broke""#),
            "error field: {json}"
        );
    }

    #[test]
    fn test_server_message_auth_required() {
        let msg = ServerMessage::AuthRequired;
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"auth_required""#),
            "type tag: {json}"
        );
    }

    #[test]
    fn test_server_message_auth_ok() {
        let msg = ServerMessage::AuthOk {
            token: "tok123".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"auth_ok""#), "type tag: {json}");
        assert!(json.contains(r#""token":"tok123""#), "token: {json}");
    }

    #[test]
    fn test_server_message_auth_error() {
        let msg = ServerMessage::AuthError {
            error: "bad password".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"auth_error""#), "type tag: {json}");
    }

    #[test]
    fn test_server_message_model_list() {
        let msg = ServerMessage::ModelList {
            models: vec!["a".into(), "b".into()],
            current: "a".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"model_list""#), "type tag: {json}");
        assert!(json.contains(r#""models""#), "models: {json}");
        assert!(json.contains(r#""current""#), "current: {json}");
    }

    #[test]
    fn test_server_message_model_changed() {
        let msg = ServerMessage::ModelChanged {
            provider_name: "anthropic".into(),
            model_name: "claude-sonnet".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"model_changed""#),
            "type tag: {json}"
        );
        assert!(
            json.contains(r#""providerName""#),
            "providerName camelCase: {json}"
        );
        assert!(
            json.contains(r#""modelName""#),
            "modelName camelCase: {json}"
        );
    }

    #[test]
    fn test_server_message_config_values() {
        let msg = ServerMessage::ConfigValues {
            config: serde_json::json!({"key": "val"}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"config_values""#),
            "type tag: {json}"
        );
    }

    #[test]
    fn test_server_message_config_saved() {
        let msg = ServerMessage::ConfigSaved {
            config: serde_json::json!({}),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"config_saved""#),
            "type tag: {json}"
        );
    }

    #[test]
    fn test_server_message_mode_changed() {
        let msg = ServerMessage::ModeChanged {
            mode: "manual".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"mode_changed""#),
            "type tag: {json}"
        );
        assert!(json.contains(r#""mode":"manual""#), "mode: {json}");
    }

    #[test]
    fn test_server_message_screenshots_changed() {
        let msg = ServerMessage::ScreenshotsChanged { enabled: true };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"screenshots_changed""#),
            "type tag: {json}"
        );
        assert!(json.contains(r#""enabled":true"#), "enabled: {json}");
    }

    #[test]
    fn test_server_message_session_list() {
        let msg = ServerMessage::SessionList { sessions: vec![] };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"session_list""#),
            "type tag: {json}"
        );
        assert!(json.contains(r#""sessions":[]"#), "sessions: {json}");
    }

    #[test]
    fn test_server_message_agent_tab_created() {
        let msg = ServerMessage::AgentTabCreated {
            session_id: "tab-new".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"agent_tab_created""#),
            "type tag: {json}"
        );
        assert!(
            json.contains(r#""sessionId":"tab-new""#),
            "sessionId camelCase: {json}"
        );
    }

    #[test]
    fn test_server_message_processing_complete_skip_none() {
        let msg = ServerMessage::ProcessingComplete {
            session_id: "s1".into(),
            messages: vec![],
            checkpoints: vec![],
            context_size: None,
            tab_id: None,
            aborted: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            !json.contains("contextSize"),
            "None contextSize should be skipped: {json}"
        );
        assert!(
            !json.contains("tabId"),
            "None tabId should be skipped: {json}"
        );
        assert!(
            !json.contains("aborted"),
            "None aborted should be skipped: {json}"
        );
    }

    #[test]
    fn test_server_message_processing_complete_with_optionals() {
        let msg = ServerMessage::ProcessingComplete {
            session_id: "s1".into(),
            messages: vec![],
            checkpoints: vec![],
            context_size: Some(100000),
            tab_id: Some("tab-1".into()),
            aborted: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""contextSize":100000"#),
            "contextSize present: {json}"
        );
        assert!(json.contains(r#""tabId":"tab-1""#), "tabId present: {json}");
    }

    #[test]
    fn test_server_message_terminal_created() {
        let msg = ServerMessage::TerminalCreated {
            id: "term-1".into(),
            cols: 80,
            rows: 24,
            scrollback: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"terminal_created""#),
            "type tag: {json}"
        );
        assert!(json.contains(r#""id":"term-1""#), "id: {json}");
        assert!(json.contains(r#""cols":80"#), "cols: {json}");
        assert!(json.contains(r#""rows":24"#), "rows: {json}");
        assert!(
            !json.contains("scrollback"),
            "None scrollback should be skipped: {json}"
        );
    }

    #[test]
    fn test_server_message_terminal_created_with_scrollback() {
        let msg = ServerMessage::TerminalCreated {
            id: "term-1".into(),
            cols: 80,
            rows: 24,
            scrollback: Some("c29tZSBkYXRh".into()),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""scrollback":"c29tZSBkYXRh""#),
            "scrollback present: {json}"
        );
    }

    #[test]
    fn test_client_message_terminal_reconnect() {
        let json = r#"{"type":"terminal_reconnect","sessionId":"t1"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::TerminalReconnect { session_id } => {
                assert_eq!(session_id, "t1");
            }
            _ => panic!("Expected TerminalReconnect"),
        }
    }

    #[test]
    fn test_server_message_terminal_output() {
        let msg = ServerMessage::TerminalOutput {
            session_id: "t1".into(),
            data: "base64data".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"terminal_output""#),
            "type tag: {json}"
        );
        assert!(
            json.contains(r#""sessionId":"t1""#),
            "sessionId camelCase: {json}"
        );
    }

    #[test]
    fn test_server_message_terminal_exited() {
        let msg = ServerMessage::TerminalExited {
            session_id: "t1".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"terminal_exited""#),
            "type tag: {json}"
        );
    }

    #[test]
    fn test_server_message_terminal_sessions() {
        let msg = ServerMessage::TerminalSessions {
            sessions: vec![TerminalSessionInfoDto {
                id: "t1".into(),
                cols: 120,
                rows: 40,
                name: None,
            }],
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"terminal_sessions""#),
            "type tag: {json}"
        );
        assert!(json.contains(r#""id":"t1""#), "id: {json}");
    }

    #[test]
    fn test_server_message_worker_notification() {
        let msg = ServerMessage::WorkerNotification {
            worker_id: "w1".into(),
            worker_name: "my-worker".into(),
            status: "success".into(),
            summary: "done".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(
            json.contains(r#""type":"worker_notification""#),
            "type tag: {json}"
        );
        assert!(
            json.contains(r#""workerId":"w1""#),
            "workerId camelCase: {json}"
        );
        assert!(
            json.contains(r#""workerName":"my-worker""#),
            "workerName camelCase: {json}"
        );
    }

    // --- TerminalSessionInfoDto ---

    #[test]
    fn test_terminal_session_info_dto_serialization() {
        let dto = TerminalSessionInfoDto {
            id: "abc".into(),
            cols: 80,
            rows: 24,
            name: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""id":"abc""#), "id: {json}");
        assert!(json.contains(r#""cols":80"#), "cols: {json}");
        assert!(json.contains(r#""rows":24"#), "rows: {json}");
        assert!(!json.contains("name"), "None name should be skipped: {json}");
    }

    #[test]
    fn test_terminal_session_info_dto_with_name() {
        let dto = TerminalSessionInfoDto {
            id: "abc".into(),
            cols: 80,
            rows: 24,
            name: Some("My Terminal".into()),
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(
            json.contains(r#""name":"My Terminal""#),
            "name present: {json}"
        );
    }

    #[test]
    fn test_client_message_terminal_rename() {
        let json = r#"{"type":"terminal_rename","sessionId":"t1","name":"Dev Shell"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::TerminalRename { session_id, name } => {
                assert_eq!(session_id, "t1");
                assert_eq!(name, "Dev Shell");
            }
            _ => panic!("Expected TerminalRename"),
        }
    }
}
