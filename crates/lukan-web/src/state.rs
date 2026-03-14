use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};

use lukan_agent::{AgentLoop, WorkerNotification};
use lukan_core::config::ResolvedConfig;
use lukan_core::config::types::PermissionMode;
use lukan_core::models::events::{ApprovalResponse, PlanReviewResponse};
use tokio::sync::{Mutex, broadcast, mpsc, watch};

use crate::protocol::ServerMessage;
use crate::terminal::TerminalManager;

/// A stream event broadcast entry.
/// `tab_id` is the persisted session ID (not the tab/agent instance ID).
/// This ensures all UIs watching the same saved session see each other's streaming.
#[derive(Clone, Debug)]
pub struct StreamBroadcast {
    pub tab_id: String,
    pub json: String,
    pub origin_conn_id: usize,
}

/// Per-session agent state (one per tab/agent instance).
pub struct WebAgentSession {
    pub agent: Option<AgentLoop>,
    pub approval_tx: Option<mpsc::Sender<ApprovalResponse>>,
    pub plan_review_tx: Option<mpsc::Sender<PlanReviewResponse>>,
    pub planner_answer_tx: Option<mpsc::Sender<String>>,
    pub bg_signal_tx: Option<watch::Sender<()>>,
    /// Human-readable label for this tab (e.g. "Agent 2")
    pub label: String,
    /// The persisted ChatSession ID (6-char hex) so we can reload after agent loss.
    pub last_session_id: Option<String>,
}

impl WebAgentSession {
    pub fn new() -> Self {
        Self {
            agent: None,
            approval_tx: None,
            plan_review_tx: None,
            planner_answer_tx: None,
            bg_signal_tx: None,
            label: "Agent 1".to_string(),
            last_session_id: None,
        }
    }
}

/// Shared application state for the web server
pub struct AppState {
    /// Agent sessions keyed by session/tab ID
    pub sessions: Mutex<HashMap<String, WebAgentSession>>,
    /// Legacy singleton agent for backward-compat (used when no sessionId is provided)
    pub agent: Mutex<Option<AgentLoop>>,
    /// Resolved configuration (provider + credentials)
    pub config: Mutex<ResolvedConfig>,
    /// Which connection ID currently holds the processing lock (0 = none)
    pub processing_owner: Mutex<Option<usize>>,
    /// HMAC secret for token signing (random, generated at startup)
    pub auth_secret: String,
    /// Optional web password (None = no auth required)
    pub web_password: Option<String>,
    /// Token TTL in milliseconds
    pub token_ttl_ms: u64,
    /// Current provider name
    pub provider_name: Mutex<String>,
    /// Current model name
    pub model_name: Mutex<String>,
    /// Connection ID counter
    connection_counter: AtomicUsize,
    /// Current permission mode (watch channel for live updates to agents)
    pub permission_mode: watch::Sender<PermissionMode>,
    /// Sender half of the approval channel (sent to the agent) — legacy singleton
    pub approval_tx: Mutex<Option<mpsc::Sender<ApprovalResponse>>>,
    /// Sender half of the plan review channel — legacy singleton
    pub plan_review_tx: Mutex<Option<mpsc::Sender<PlanReviewResponse>>>,
    /// Sender half of the planner answer channel — legacy singleton
    pub planner_answer_tx: Mutex<Option<mpsc::Sender<String>>>,
    /// Sender for background signal — legacy singleton
    pub bg_signal_tx: Mutex<Option<watch::Sender<()>>>,
    /// Broadcast channel for worker notifications from the daemon
    pub notification_tx: broadcast::Sender<WorkerNotification>,
    /// Terminal PTY manager
    pub terminal_manager: TerminalManager,
    /// Broadcast channel for terminal output (sent to all WS clients)
    pub terminal_tx: broadcast::Sender<ServerMessage>,
    /// Broadcast channel for agent stream events (sent to all watchers of a session)
    pub stream_tx: broadcast::Sender<StreamBroadcast>,
    /// Maps session/tab ID → set of connection IDs watching that session
    pub session_watchers: Mutex<HashMap<String, HashSet<usize>>>,
}

impl AppState {
    pub fn new(resolved: ResolvedConfig) -> Self {
        // Generate random auth secret
        let secret_bytes: [u8; 32] = rand::random();
        let auth_secret = secret_bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();

        let web_password = resolved.config.web_password.clone();
        let token_ttl_ms = resolved.config.web_token_ttl.unwrap_or(24).max(1) * 60 * 60 * 1000;

        let provider_name = resolved.config.provider.to_string();
        let model_name = resolved.effective_model().unwrap_or_default();
        let (notification_tx, _) = broadcast::channel(64);
        let (terminal_tx, _) = broadcast::channel(256);
        let (stream_tx, _) = broadcast::channel(512);

        Self {
            sessions: Mutex::new(HashMap::new()),
            agent: Mutex::new(None),
            config: Mutex::new(resolved),
            processing_owner: Mutex::new(None),
            auth_secret,
            web_password,
            token_ttl_ms,
            provider_name: Mutex::new(provider_name),
            model_name: Mutex::new(model_name),
            connection_counter: AtomicUsize::new(1),
            permission_mode: watch::Sender::new(PermissionMode::Auto),
            approval_tx: Mutex::new(None),
            plan_review_tx: Mutex::new(None),
            planner_answer_tx: Mutex::new(None),
            bg_signal_tx: Mutex::new(None),
            notification_tx,
            terminal_manager: TerminalManager::default(),
            terminal_tx,
            stream_tx,
            session_watchers: Mutex::new(HashMap::new()),
        }
    }

    /// Get the next unique connection ID
    pub fn next_connection_id(&self) -> usize {
        self.connection_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// Check if authentication is required
    pub fn auth_required(&self) -> bool {
        self.web_password.is_some()
    }

    /// Validate a password and return a token if correct
    pub fn validate_password(&self, password: &str) -> Option<String> {
        if let Some(ref expected) = self.web_password {
            if password == expected {
                Some(crate::auth::create_auth_token(
                    &self.auth_secret,
                    self.token_ttl_ms,
                ))
            } else {
                None
            }
        } else {
            // No password required, always return a token
            Some(crate::auth::create_auth_token(
                &self.auth_secret,
                self.token_ttl_ms,
            ))
        }
    }

    /// Verify an auth token
    pub fn verify_token(&self, token: &str) -> bool {
        crate::auth::verify_auth_token(token, &self.auth_secret)
    }
}

// Need Arc wrapper for axum state
impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("auth_required", &self.auth_required())
            .finish()
    }
}
