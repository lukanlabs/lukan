use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use lukan_agent::{AgentLoop, PipelineNotification, WorkerNotification};
use lukan_core::config::ResolvedConfig;
use lukan_core::config::types::PermissionMode;
use lukan_core::models::events::{ApprovalResponse, PlanReviewResponse};
use tokio::sync::{Mutex, broadcast, mpsc, watch};
use tokio_util::sync::CancellationToken;

use crate::protocol::ServerMessage;
use crate::terminal::TerminalManager;

/// A stream event broadcast entry sent to all connected clients.
#[derive(Clone, Debug)]
pub struct StreamBroadcast {
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
    /// Tools disabled via Alt+P in TUI (applied when agent is created)
    pub disabled_tools: std::collections::HashSet<String>,
}

impl WebAgentSession {
    /// Set the agent and apply any pending disabled_tools
    pub fn set_agent(&mut self, mut agent: AgentLoop) {
        if !self.disabled_tools.is_empty() {
            agent.set_disabled_tools(self.disabled_tools.clone());
        }
        self.agent = Some(agent);
    }

    pub fn new() -> Self {
        Self {
            agent: None,
            approval_tx: None,
            plan_review_tx: None,
            planner_answer_tx: None,
            bg_signal_tx: None,
            label: "Agent 1".to_string(),
            last_session_id: None,
            disabled_tools: std::collections::HashSet::new(),
        }
    }
}

/// Shared application state for the web server
pub struct AppState {
    /// Agent sessions keyed by session/tab ID
    pub sessions: Mutex<HashMap<String, WebAgentSession>>,
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
    /// Broadcast channel for worker notifications from the daemon
    pub notification_tx: broadcast::Sender<WorkerNotification>,
    /// Broadcast channel for pipeline notifications from the daemon
    pub pipeline_notification_tx: broadcast::Sender<PipelineNotification>,
    /// Terminal PTY manager
    pub terminal_manager: TerminalManager,
    /// Broadcast channel for terminal output (sent to all WS clients)
    pub terminal_tx: broadcast::Sender<ServerMessage>,
    /// Broadcast channel for agent stream events (sent to all connected clients)
    pub stream_tx: broadcast::Sender<StreamBroadcast>,
    /// Cancellation tokens for running pipeline executions (keyed by pipeline_id)
    pub pipeline_cancel_tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
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
        let (pipeline_notification_tx, _) = broadcast::channel(64);
        let (terminal_tx, _) = broadcast::channel(256);
        let (stream_tx, _) = broadcast::channel(512);

        // Load permission mode from project config (if available)
        let initial_mode = {
            let cwd = std::env::current_dir().unwrap_or_default();
            load_permission_mode_sync(&cwd)
        };

        Self {
            sessions: Mutex::new(HashMap::new()),
            config: Mutex::new(resolved),
            processing_owner: Mutex::new(None),
            auth_secret,
            web_password,
            token_ttl_ms,
            provider_name: Mutex::new(provider_name),
            model_name: Mutex::new(model_name),
            connection_counter: AtomicUsize::new(1),
            permission_mode: watch::Sender::new(initial_mode),
            notification_tx,
            pipeline_notification_tx,
            terminal_manager: TerminalManager::default(),
            terminal_tx,
            stream_tx,
            pipeline_cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
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

/// Load permission mode from .lukan/config.json (sync, for startup)
fn load_permission_mode_sync(start_dir: &std::path::Path) -> PermissionMode {
    let mut dir = start_dir.to_path_buf();
    loop {
        let config_path = dir.join(".lukan").join("config.json");
        if let Ok(content) = std::fs::read_to_string(&config_path)
            && let Ok(cfg) = serde_json::from_str::<lukan_core::config::ProjectConfig>(&content)
        {
            return cfg.permission_mode;
        }
        if !dir.pop() {
            break;
        }
    }
    PermissionMode::Auto
}

// Need Arc wrapper for axum state
impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("auth_required", &self.auth_required())
            .finish()
    }
}
