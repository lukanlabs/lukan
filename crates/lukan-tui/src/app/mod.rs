mod event_agent;
mod helpers;
mod keys;
mod model;
mod render;
mod session;
mod stream;
mod submit;

use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    event::KeyCode,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget},
};
use std::collections::{HashMap, HashSet};
use std::io::{Stdout, stdout};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use tracing::error;

use lukan_agent::sub_agent::{SubAgentUpdate, get_all_sub_agents, subscribe_updates};
use lukan_agent::{AgentConfig, AgentLoop, NotificationWatcher, SessionManager};
use lukan_core::config::types::PermissionMode;
use lukan_core::config::{
    ConfigManager, CredentialsManager, LukanPaths, ProviderName, ResolvedConfig,
};
use lukan_core::models::events::{
    ApprovalResponse, PlanReviewResponse, PlanTask, PlannerQuestionItem, StopReason, StreamEvent,
    ToolApprovalRequest,
};
use lukan_core::models::sessions::SessionSummary;
use lukan_core::workers::WorkerManager;
use lukan_providers::{Provider, SystemPrompt, create_provider};
use lukan_tools::{all_tool_names, create_configured_browser_registry, create_configured_registry};

use chrono::Utc;

use crate::event::{AppEvent, is_quit, spawn_event_reader};
use crate::widgets::approval_prompt::{ApprovalPromptWidget, summarize_tool_input};
use crate::widgets::bg_picker::{BgEntry, BgPicker, BgPickerView, BgPickerWidget};
use crate::widgets::chat::{
    ChatMessage, ChatWidget, build_message_lines, physical_row_count, sanitize_for_display,
};
use crate::widgets::command_palette::CommandPaletteWidget;
use crate::widgets::event_picker::{EventPicker, EventPickerMode, EventPickerWidget};
use crate::widgets::input::{InputWidget, cursor_position, input_height};
use crate::widgets::model_palette::ModelPaletteWidget;
use crate::widgets::plan_review::PlanReviewWidget;
use crate::widgets::planner_question::PlannerQuestionWidget;
use crate::widgets::reasoning_palette::ReasoningPaletteWidget;
use crate::widgets::rewind_picker::{RewindEntry, RewindPicker, RewindPickerWidget, RewindView};
use crate::widgets::session_picker::SessionPickerWidget;
use crate::widgets::status_bar::StatusBarWidget;
use crate::widgets::subagent_picker::{
    SubAgentDisplayEntry, SubAgentPicker, SubAgentPickerView, SubAgentPickerWidget,
};
use crate::widgets::task_panel::TaskPanelWidget;
use crate::widgets::terminal::TerminalWidget;
use crate::widgets::tool_picker::ToolPickerWidget;
use crate::widgets::trust_prompt::TrustPromptWidget;
use crate::widgets::worker_picker::{
    RunEntry, WorkerEntry, WorkerPicker, WorkerPickerView, WorkerPickerWidget,
};

use crate::terminal_modal::TerminalModal;

/// Which view the TUI is currently showing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveView {
    Main,
    EventAgent,
}

/// Application state
pub struct App {
    messages: Vec<ChatMessage>,
    input: String,
    cursor_pos: usize,
    streaming_text: String,
    /// Accumulated thinking/reasoning text from the model (separate from response text)
    streaming_thinking: String,
    is_streaming: bool,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    /// Context size of the last API call (= input_tokens of the latest turn)
    context_size: u64,
    provider: Arc<dyn Provider>,
    config: ResolvedConfig,
    should_quit: bool,
    /// Model picker state
    model_picker: Option<ModelPicker>,
    /// Session picker state
    session_picker: Option<SessionPicker>,
    /// Persistent agent loop (maintains history across messages)
    agent: Option<AgentLoop>,
    /// Channel to receive agent back after a turn completes
    agent_return_rx: Option<tokio::sync::oneshot::Receiver<AgentLoop>>,
    /// Current tool being executed (for status display)
    active_tool: Option<String>,
    /// Current session ID for display
    session_id: Option<String>,
    /// Index of the first message NOT yet committed to the terminal scrollback.
    /// Messages before this index have already been pushed via `insert_before`.
    committed_msg_idx: usize,
    /// Number of wrapped rows (from uncommitted messages + streaming) that have
    /// already been pushed to terminal scrollback.  Resets to 0 when
    /// `committed_msg_idx` advances past fully-scrolled messages.
    viewport_scroll: u16,
    /// Selected index in the command palette (when typing `/`)
    cmd_palette_idx: usize,
    /// Reasoning effort picker state (shown after selecting a codex model)
    reasoning_picker: Option<ReasoningPicker>,
    /// Force full terminal redraw on next frame (e.g. after closing overlay)
    force_redraw: bool,
    /// Queued messages ready to submit (set by handle_stream_event, consumed by main loop)
    pending_queue_submit: bool,
    /// First ESC was pressed — show hint and clear on second ESC
    esc_pending: bool,
    /// When set: (start_byte, end_byte, preview_label). Paste block inside self.input.
    paste_info: Option<(usize, usize, String)>,
    /// Current position in input history (indexes into user messages from self.messages)
    history_idx: Option<usize>,
    /// Saved current input when browsing history
    history_saved_input: String,
    /// Background process picker state
    bg_picker: Option<BgPicker>,
    /// Worker picker overlay state
    worker_picker: Option<WorkerPicker>,
    /// SubAgent picker overlay state (Alt+S)
    subagent_picker: Option<SubAgentPicker>,
    /// Rewind (checkpoint restore) picker state
    rewind_picker: Option<RewindPicker>,
    /// Memory viewer overlay content (shown via Alt+M)
    memory_viewer: Option<String>,
    /// Scroll offset for memory viewer
    memory_viewer_scroll: u16,
    /// Trust prompt overlay (shown on first launch in an untrusted directory)
    trust_prompt: Option<TrustPrompt>,
    /// Sender to signal Alt+B (send running Bash to background)
    bg_signal_tx: watch::Sender<()>,
    /// Receiver half (cloned into AgentConfig)
    bg_signal_rx: watch::Receiver<()>,
    /// Cancellation token for the current agent turn (ESC to cancel)
    cancel_token: Option<CancellationToken>,
    /// Current permission mode
    permission_mode: PermissionMode,
    /// Sender half of the approval channel (cloned into AgentConfig)
    approval_tx: Option<mpsc::Sender<ApprovalResponse>>,
    /// Approval prompt overlay state
    approval_prompt: Option<ApprovalPrompt>,
    /// Plan review overlay state
    plan_review: Option<PlanReviewState>,
    /// Sender to respond to plan review
    plan_review_tx: Option<mpsc::Sender<PlanReviewResponse>>,
    /// Planner question overlay state
    planner_question: Option<PlannerQuestionState>,
    /// Sender to respond to planner questions
    planner_answer_tx: Option<mpsc::Sender<String>>,
    /// Messages queued while the agent was streaming (shared with agent)
    queued_messages: Arc<std::sync::Mutex<Vec<String>>>,
    /// Whether the task panel is visible (toggled with Alt+T)
    task_panel_visible: bool,
    /// Cached task entries for the task panel
    task_panel_entries: Vec<lukan_tools::tasks::TaskEntry>,
    /// Flag to trigger task panel refresh after task tool events
    task_panel_needs_refresh: bool,
    /// Toast notifications (message, created_at) — auto-expire after a few seconds
    toast_notifications: Vec<(String, Instant)>,
    /// Watcher for worker daemon notifications (JSONL file)
    notification_watcher: NotificationWatcher,
    /// Last time we polled pending.jsonl for system events
    last_event_poll: Instant,
    /// Buffered system events waiting to be pushed to agent
    event_buffer: Vec<(String, String, String)>,
    /// Whether browser tools are enabled (--browser flag)
    browser_tools: bool,
    /// MCP server manager (kept alive for tool proxies)
    mcp_manager: Option<lukan_tools::mcp::McpManager>,
    /// Currently active view (Main or EventAgent)
    active_view: ActiveView,
    /// The Event Agent sub-agent (autonomous, Skip mode)
    event_agent: Option<AgentLoop>,
    /// Channel to receive event agent back after a turn completes
    event_agent_return_rx: Option<tokio::sync::oneshot::Receiver<AgentLoop>>,
    /// Messages in the Event Agent view
    event_messages: Vec<ChatMessage>,
    /// Streaming text for the Event Agent
    event_streaming_text: String,
    /// Whether the Event Agent is currently streaming
    event_is_streaming: bool,
    /// Current tool being executed by the Event Agent
    event_active_tool: Option<String>,
    /// Token usage for the Event Agent
    event_input_tokens: u64,
    event_output_tokens: u64,
    /// Committed message index for Event Agent scrollback
    event_committed_msg_idx: usize,
    /// Row-level scroll for event agent viewport
    event_viewport_scroll: u16,
    /// Whether the Event Agent has unread messages (badge)
    event_agent_has_unread: bool,
    /// Index of the first assistant text message in the current streaming turn.
    /// Continuation text after tool calls is appended here instead of creating
    /// a separate message (prevents mid-sentence splits).
    turn_text_msg_idx: Option<usize>,
    /// Runtime tool picker overlay state (Alt+P)
    tool_picker: Option<ToolPicker>,
    /// Persisted disabled tools applied to new turns
    disabled_tools: HashSet<String>,
    /// Unified event picker / log viewer (Alt+L)
    event_picker: Option<EventPicker>,
    /// Auto (true) vs manual (false) event forwarding mode
    event_auto_mode: bool,
    /// Auto-load the most recent session on startup (--continue flag)
    continue_session: bool,
    /// Embedded interactive terminal modal (F9)
    terminal_modal: Option<TerminalModal>,
    /// Whether the terminal modal overlay is visible (false = minimized)
    terminal_visible: bool,
    /// Whether mouse capture is currently enabled (for terminal selection)
    mouse_capture_enabled: bool,
    /// Cached inner area of the terminal overlay (set during render, used for mouse hit-testing)
    terminal_overlay_inner: Option<ratatui::layout::Rect>,
    /// Daemon WebSocket sender (Some = daemon mode, None = in-process agent)
    daemon_tx: Option<crate::ws_client::DaemonSender>,
    /// Daemon WebSocket event receiver
    daemon_rx: Option<mpsc::UnboundedReceiver<crate::ws_client::DaemonEvent>>,
    /// Port of the daemon server (0 = not using daemon)
    daemon_port: u16,
    /// Tab ID on the daemon (TUI's own agent tab)
    daemon_tab_id: Option<String>,
}

/// Trust prompt state — shown when the user hasn't trusted this workspace yet
pub(crate) struct TrustPrompt {
    pub(crate) cwd: String,
    pub(crate) selected: usize, // 0 = Yes, 1 = No
}

/// Approval prompt overlay — shown when tools need user approval
pub(crate) struct ApprovalPrompt {
    pub(crate) tools: Vec<ToolApprovalRequest>,
    /// Per-tool toggle (default all true = approved)
    pub(crate) selections: Vec<bool>,
    /// Cursor position
    pub(crate) selected: usize,
    /// Whether all tools in this prompt are read-only according to metadata.
    pub(crate) all_read_only: bool,
}

/// Interactive model picker state
pub(crate) struct ModelPicker {
    pub(crate) models: Vec<String>,
    pub(crate) selected: usize,
    pub(crate) current: String,
}

/// Reasoning effort picker state
pub(crate) struct ReasoningPicker {
    pub(crate) model_entry: String,
    pub(crate) levels: Vec<(&'static str, &'static str, &'static str)>, // (value, label, description)
    pub(crate) selected: usize,
}

/// Interactive session picker state
pub(crate) struct SessionPicker {
    pub(crate) sessions: Vec<SessionSummary>,
    pub(crate) selected: usize,
    pub(crate) current_id: Option<String>,
}

pub(crate) struct ToolGroup {
    pub(crate) name: String,
    pub(crate) tools: Vec<String>,
}

pub(crate) struct ToolPicker {
    pub(crate) groups: Vec<ToolGroup>,
    pub(crate) selected: usize,
    pub(crate) disabled: HashSet<String>,
}

const BROWSER_TOOLS: &[&str] = &[
    "BrowserNavigate",
    "BrowserSnapshot",
    "BrowserScreenshot",
    "BrowserClick",
    "BrowserType",
    "BrowserEvaluate",
    "BrowserSavePDF",
    "BrowserTabs",
    "BrowserNewTab",
    "BrowserSwitchTab",
];

/// Plan review overlay mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanReviewMode {
    /// Viewing task list
    List,
    /// Viewing detail for selected task
    Detail,
    /// Typing feedback text
    Feedback,
}

/// Plan review overlay state — shown when agent submits a plan
pub(crate) struct PlanReviewState {
    #[allow(dead_code)]
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) plan: String,
    pub(crate) tasks: Vec<PlanTask>,
    pub(crate) selected: usize,
    pub(crate) mode: PlanReviewMode,
    pub(crate) feedback_input: String,
    #[allow(dead_code)]
    pub(crate) scroll: u16,
}

/// Planner question overlay state
pub(crate) struct PlannerQuestionState {
    #[allow(dead_code)]
    pub(crate) id: String,
    pub(crate) questions: Vec<PlannerQuestionItem>,
    pub(crate) current_question: usize,
    /// Selected option index per question (last index = custom input)
    pub(crate) selections: Vec<usize>,
    /// For multi-select: which options are toggled per question
    pub(crate) multi_selections: Vec<Vec<bool>>,
    /// Whether we're in text-input mode for the custom response
    pub(crate) editing_custom: bool,
    /// Custom text input per question
    pub(crate) custom_inputs: Vec<String>,
}

impl App {
    pub fn new(provider: Box<dyn Provider>, config: ResolvedConfig) -> Self {
        let provider = Arc::from(provider);
        let (bg_signal_tx, bg_signal_rx) = watch::channel(());

        let disabled_tools = config
            .config
            .disabled_tools
            .as_ref()
            .map(|d| d.iter().cloned().collect::<HashSet<_>>())
            .unwrap_or_default();

        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            streaming_text: String::new(),
            streaming_thinking: String::new(),
            is_streaming: false,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_creation_tokens: 0,
            context_size: 0,
            provider,
            config,
            should_quit: false,
            model_picker: None,
            session_picker: None,
            agent: None,
            agent_return_rx: None,
            active_tool: None,
            session_id: None,
            committed_msg_idx: 0,
            viewport_scroll: 0,
            cmd_palette_idx: 0,
            reasoning_picker: None,
            force_redraw: false,
            pending_queue_submit: false,
            esc_pending: false,
            paste_info: None,
            history_idx: None,
            history_saved_input: String::new(),
            bg_picker: None,
            worker_picker: None,
            subagent_picker: None,
            rewind_picker: None,
            memory_viewer: None,
            memory_viewer_scroll: 0,
            trust_prompt: None,
            bg_signal_tx,
            bg_signal_rx,
            cancel_token: None,
            permission_mode: PermissionMode::Auto,
            approval_tx: None,
            approval_prompt: None,
            plan_review: None,
            plan_review_tx: None,
            planner_question: None,
            planner_answer_tx: None,
            queued_messages: Arc::new(std::sync::Mutex::new(Vec::new())),
            task_panel_visible: false,
            task_panel_entries: Vec::new(),
            task_panel_needs_refresh: false,
            toast_notifications: Vec::new(),
            notification_watcher: NotificationWatcher::new(),
            last_event_poll: Instant::now(),
            event_buffer: Vec::new(),
            browser_tools: false,
            mcp_manager: None,
            active_view: ActiveView::Main,
            event_agent: None,
            event_agent_return_rx: None,
            event_messages: Vec::new(),
            event_streaming_text: String::new(),
            event_is_streaming: false,
            event_active_tool: None,
            event_input_tokens: 0,
            event_output_tokens: 0,
            event_committed_msg_idx: 0,
            event_viewport_scroll: 0,
            event_agent_has_unread: false,
            turn_text_msg_idx: None,
            tool_picker: None,
            disabled_tools,
            event_picker: None,
            event_auto_mode: false,
            continue_session: false,
            terminal_modal: None,
            terminal_visible: false,
            mouse_capture_enabled: false,
            terminal_overlay_inner: None,
            daemon_tx: None,
            daemon_rx: None,
            daemon_port: 0,
            daemon_tab_id: None,
        }
    }

    /// Create an App that connects to the daemon via WebSocket.
    /// Falls back to in-process agent mode if the connection fails.
    pub async fn new_daemon(
        provider: Box<dyn Provider>,
        config: ResolvedConfig,
        port: u16,
    ) -> Self {
        let mut app = Self::new(provider, config);
        app.daemon_port = port;

        match crate::ws_client::connect(port).await {
            Ok((tx, rx, tab_id)) => {
                tracing::info!(port, tab_id = %tab_id, "TUI connected to daemon");
                app.daemon_tab_id = Some(tab_id);
                app.daemon_tx = Some(tx);
                app.daemon_rx = Some(rx);
            }
            Err(e) => {
                tracing::warn!(port, error = %e, "Failed to connect to daemon, using in-process agent");
            }
        }

        app
    }

    /// Whether we're connected to the daemon (vs in-process agent mode).
    fn is_daemon_mode(&self) -> bool {
        self.daemon_tx.is_some()
    }

    /// Mark this app to auto-load the most recent session on startup.
    pub fn set_continue_session(&mut self) {
        self.continue_session = true;
    }

    /// Enable browser tools for this session.
    pub fn enable_browser_tools(&mut self) {
        self.browser_tools = true;
    }

    fn build_tool_groups(&self) -> Vec<ToolGroup> {
        let mut groups: Vec<ToolGroup> = Vec::new();

        let mut plugin_tool_names: HashSet<String> = HashSet::new();
        let plugins_dir = LukanPaths::plugins_dir();
        if let Ok(entries) = std::fs::read_dir(&plugins_dir) {
            let mut plugin_groups: Vec<ToolGroup> = Vec::new();
            for entry in entries.flatten() {
                let plugin_dir = entry.path();
                if !plugin_dir.is_dir() {
                    continue;
                }
                let plugin_name = match plugin_dir.file_name().and_then(|n| n.to_str()) {
                    Some(name) => name.to_string(),
                    None => continue,
                };
                let tools_path = plugin_dir.join("tools.json");
                if !tools_path.exists() {
                    continue;
                }
                let content = match std::fs::read_to_string(&tools_path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let parsed: serde_json::Value = match serde_json::from_str(&content) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let mut names: Vec<String> = parsed
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.get("name").and_then(|v| v.as_str()))
                    .map(|name| name.to_string())
                    .collect();
                names.sort();
                names.dedup();
                if names.is_empty() {
                    continue;
                }
                plugin_tool_names.extend(names.iter().cloned());
                plugin_groups.push(ToolGroup {
                    name: plugin_name,
                    tools: names,
                });
            }
            plugin_groups.sort_by(|a, b| a.name.cmp(&b.name));
            groups.extend(plugin_groups);
        }

        let browser_tools: HashSet<String> =
            BROWSER_TOOLS.iter().map(|t| (*t).to_string()).collect();
        if self.browser_tools {
            let mut browser: Vec<String> = BROWSER_TOOLS.iter().map(|t| (*t).to_string()).collect();
            browser.sort();
            if !browser.is_empty() {
                groups.push(ToolGroup {
                    name: "Browser".to_string(),
                    tools: browser,
                });
            }
        }

        let mut core: Vec<String> = all_tool_names()
            .into_iter()
            .filter(|name| !plugin_tool_names.contains(name))
            .filter(|name| !browser_tools.contains(name))
            .collect();
        core.sort();
        if !core.is_empty() {
            groups.push(ToolGroup {
                name: "Core".to_string(),
                tools: core,
            });
        }

        groups.sort_by(|a, b| a.name.cmp(&b.name));
        groups
    }

    fn tool_picker_tool_count(picker: &ToolPicker) -> usize {
        picker.groups.iter().map(|g| g.tools.len()).sum()
    }

    fn tool_picker_selected_tool_name(picker: &ToolPicker) -> Option<String> {
        let mut idx = 0usize;
        for group in &picker.groups {
            for tool in &group.tools {
                if idx == picker.selected {
                    return Some(tool.clone());
                }
                idx += 1;
            }
        }
        None
    }

    fn open_tool_picker(&mut self) {
        let groups = self.build_tool_groups();
        let selected = 0usize;
        self.tool_picker = Some(ToolPicker {
            groups,
            selected,
            disabled: self.disabled_tools.clone(),
        });
        self.force_redraw = true;
    }

    fn close_tool_picker(&mut self) {
        if let Some(picker) = self.tool_picker.take() {
            self.disabled_tools = picker.disabled;
            if let Some(ref mut agent) = self.agent {
                agent.set_disabled_tools(self.disabled_tools.clone());
            }
            if let Some(ref mut event_agent) = self.event_agent {
                event_agent.set_disabled_tools(self.disabled_tools.clone());
            }
            // In daemon mode, send disabled tools to server
            if let Some(ref daemon) = self.daemon_tx {
                let tools: Vec<String> = self.disabled_tools.iter().cloned().collect();
                let _ = daemon.send(&crate::ws_client::OutMessage::SetDisabledTools {
                    tools,
                    session_id: self.daemon_tab_id.clone(),
                });
            }
        }
        self.force_redraw = true;
    }

    /// Build the display text for the input widget.
    /// Shows: text_before + [Pasted Content N chars] + text_after
    fn display_input(&self) -> String {
        if let Some((start, end, ref label)) = self.paste_info {
            let before = &self.input[..start];
            let after = &self.input[end..];
            format!("{before}{label}{after}")
        } else {
            // Replace newlines with spaces for single-line display
            // (newlines are preserved in self.input for sending to the agent)
            self.input.replace('\n', " ")
        }
    }

    /// Convert a byte position in self.input to a byte position in the display string.
    fn display_cursor(&self) -> usize {
        if let Some((start, end, ref label)) = self.paste_info {
            if self.cursor_pos <= start {
                // Before paste block — maps 1:1
                self.cursor_pos
            } else if self.cursor_pos < end {
                // Inside paste block — show at start of label
                start + label.len()
            } else {
                // After paste block — offset by (label.len - paste_len)
                let paste_len = end - start;
                self.cursor_pos - paste_len + label.len()
            }
        } else {
            self.cursor_pos
        }
    }

    pub fn current_session_id(&self) -> Option<String> {
        self.session_id.clone()
    }

    /// Create a new AgentLoop with a fresh session
    async fn create_agent(&mut self) -> AgentLoop {
        let system_prompt = helpers::build_system_prompt_with_opts(self.browser_tools).await;

        let cwd = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());

        // Load project config for permissions and allowed paths
        let project_cfg = lukan_core::config::ProjectConfig::load(&cwd)
            .await
            .ok()
            .flatten()
            .map(|(_, cfg)| cfg);

        let permissions = project_cfg
            .as_ref()
            .map(|c| c.permissions.clone())
            .unwrap_or_default();

        let allowed = project_cfg
            .as_ref()
            .map(|c| c.resolve_allowed_paths(&cwd))
            .unwrap_or_else(|| vec![cwd.clone()]);

        // NOTE: permission_mode is loaded once from project config in run(),
        // not here, so user overrides via Shift+Tab are preserved across agent
        // recreation (/clear, new session, etc.).

        // Create approval channel
        let (approval_tx, approval_rx) = mpsc::channel::<ApprovalResponse>(1);
        self.approval_tx = Some(approval_tx);

        // Create plan review channel
        let (plan_review_tx, plan_review_rx) = mpsc::channel::<PlanReviewResponse>(1);
        self.plan_review_tx = Some(plan_review_tx);

        // Create planner answer channel
        let (planner_answer_tx, planner_answer_rx) = mpsc::channel::<String>(1);
        self.planner_answer_tx = Some(planner_answer_tx);

        let mut tools = if self.browser_tools {
            create_configured_browser_registry(&permissions, &allowed)
        } else {
            create_configured_registry(&permissions, &allowed)
        };

        // Register MCP tools if configured
        if !self.config.config.mcp_servers.is_empty() {
            let result =
                lukan_tools::init_mcp_tools(&mut tools, &self.config.config.mcp_servers).await;
            if result.tool_count > 0 {
                tracing::info!(count = result.tool_count, "MCP tools registered");
            }
            for (server, err) in &result.errors {
                tracing::warn!(server = %server, "MCP error: {err}");
            }
            self.mcp_manager = Some(result.manager);
        }

        let config = AgentConfig {
            provider: Arc::clone(&self.provider),
            tools,
            system_prompt,
            cwd,
            provider_name: self.config.config.provider.to_string(),
            model_name: self.config.effective_model().unwrap_or_default(),
            bg_signal: Some(self.bg_signal_rx.clone()),
            allowed_paths: Some(allowed),
            permission_mode: self.permission_mode.clone(),
            permission_mode_rx: None,
            permissions,
            approval_rx: Some(approval_rx),
            plan_review_rx: Some(plan_review_rx),
            planner_answer_rx: Some(planner_answer_rx),
            browser_tools: self.browser_tools,
            skip_session_save: false,
            vision_provider: lukan_providers::create_vision_provider(
                self.config.config.vision_model.as_deref(),
                &self.config.credentials,
            )
            .map(std::sync::Arc::from),
            extra_env: self.config.credentials.flatten_skill_env(),
            compaction_threshold: self
                .config
                .config
                .model_settings
                .get(&self.config.effective_model().unwrap_or_default())
                .and_then(|s| s.compaction_threshold),
            tab_id: self.daemon_tab_id.clone(),
        };

        let blocked_env_vars = project_cfg
            .as_ref()
            .map(|c| c.blocked_env_vars.clone())
            .unwrap_or_default();

        match AgentLoop::new(config).await {
            Ok(mut agent) => {
                agent.set_disabled_tools(self.disabled_tools.clone());
                agent.set_blocked_env_vars(blocked_env_vars);
                agent
            }
            Err(e) => {
                // Fallback: if session creation fails, log error and panic
                // This shouldn't happen in normal operation
                panic!("Failed to create agent session: {e}");
            }
        }
    }

    /// Enable or disable mouse capture based on terminal modal visibility.
    fn sync_mouse_capture(&mut self, enabled: bool) {
        if enabled && !self.mouse_capture_enabled {
            let _ = stdout().execute(crossterm::event::EnableMouseCapture);
            self.mouse_capture_enabled = true;
        } else if !enabled && self.mouse_capture_enabled {
            let _ = stdout().execute(crossterm::event::DisableMouseCapture);
            self.mouse_capture_enabled = false;
        }
    }

    /// Copy text to system clipboard via xclip.
    /// Copy text to both PRIMARY and CLIPBOARD X11 selections.
    fn copy_to_clipboard(text: &str) {
        use std::process::{Command, Stdio};
        // Copy to both selections so middle-click paste and Ctrl+V both work
        for selection in ["clipboard", "primary"] {
            if let Ok(mut child) = Command::new("xclip")
                .args(["-selection", selection])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                // Must take() stdin to close the pipe, otherwise xclip waits for EOF forever
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = std::io::Write::write_all(&mut stdin, text.as_bytes());
                }
                // stdin is dropped here → pipe closed → xclip reads EOF and sets selection
                let _ = child.wait();
            } else if let Ok(mut child) = Command::new("xsel")
                .arg(if selection == "primary" {
                    "--primary"
                } else {
                    "--clipboard"
                })
                .arg("--input")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = std::io::Write::write_all(&mut stdin, text.as_bytes());
                }
                let _ = child.wait();
            }
        }
    }

    pub async fn run(mut self) -> Result<()> {
        // Ensure cursor starts at column 0 before creating the inline viewport,
        // otherwise the first rendered line inherits the shell prompt's column offset.
        stdout().execute(crossterm::cursor::MoveToColumn(0))?;
        enable_raw_mode()?;
        // Enable bracketed paste so we get Paste events instead of individual keystrokes
        stdout().execute(crossterm::event::EnableBracketedPaste)?;
        // NO AlternateScreen, NO EnableMouseCapture — we use inline viewport
        // so content scrolls into the native terminal scrollback.
        let backend = CrosstermBackend::new(stdout());
        let size = crossterm::terminal::size()?;
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Inline(size.1),
            },
        )?;

        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AppEvent>();
        spawn_event_reader(event_tx);

        let (agent_tx, mut agent_rx) = mpsc::channel::<StreamEvent>(256);
        let (event_agent_tx, mut event_agent_rx) = mpsc::channel::<StreamEvent>(256);

        // Check workspace trust and load permission mode from project config
        let cwd = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());
        let project_cfg = lukan_core::config::ProjectConfig::load(&cwd)
            .await
            .ok()
            .flatten();
        let is_trusted = project_cfg.as_ref().is_some_and(|(_, cfg)| cfg.trusted);
        if let Some((_, ref cfg)) = project_cfg {
            self.permission_mode = cfg.permission_mode.clone();
        }
        if !is_trusted {
            self.trust_prompt = Some(TrustPrompt {
                cwd: cwd.display().to_string(),
                selected: 0,
            });
        }

        // Subscribe to real-time sub-agent updates
        let mut subagent_update_rx = subscribe_updates().await;

        // Welcome banner
        self.messages.push(ChatMessage::new(
            "banner",
            helpers::build_welcome_banner(
                self.provider.name(),
                &self
                    .config
                    .effective_model()
                    .unwrap_or_else(|| "(no model selected)".to_string()),
            ),
        ));

        // Auto-load most recent session if --continue was passed
        let continue_cwd = cwd.to_string_lossy().to_string();
        if self.continue_session
            && let Ok(sessions) = SessionManager::list_for_cwd(&continue_cwd).await
        {
            if let Some(most_recent) = sessions.first() {
                let session_id = most_recent.id.clone();
                // Temporarily set a session picker so load_selected_session works
                self.session_picker = Some(SessionPicker {
                    sessions,
                    selected: 0,
                    current_id: None,
                });
                self.load_selected_session(0).await;
                self.session_picker = None;
                self.force_redraw = true;
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Resumed session {session_id}"),
                ));
            } else {
                self.messages.push(ChatMessage::new(
                    "system",
                    "No previous sessions to continue.",
                ));
            }
        }

        loop {
            // Push overflow messages into the terminal scrollback
            // (skip when session picker is open — insert_before shifts the
            // viewport and leaves visual artifacts over the picker overlay)
            let term_size = terminal.size()?;
            let display_input = self.display_input();
            let cur_input_h = input_height(&display_input, term_size.width, 8);
            let task_panel_h: u16 = if self.task_panel_visible {
                if self.task_panel_entries.is_empty() {
                    3 // border top + "No tasks" + border bottom
                } else {
                    (self.task_panel_entries.len() as u16 + 2).min(8)
                }
            } else {
                0
            };
            // Account for all fixed-height areas to match the actual Layout:
            // status_bar(1) + margin(1) + shimmer(0|1) + queued(0|1)
            let view_streaming = match self.active_view {
                ActiveView::Main => self.is_streaming,
                ActiveView::EventAgent => self.event_is_streaming,
            };
            let shimmer_h: u16 = if view_streaming { 1 } else { 0 };
            let queued_h: u16 = if !self.queued_messages.lock().unwrap().is_empty() {
                1
            } else {
                0
            };
            let chat_area_h = term_size
                .height
                .saturating_sub(cur_input_h + 2 + task_panel_h + shimmer_h + queued_h);

            // Sync terminal PTY size with actual overlay inner dimensions
            if let Some(ref mut modal) = self.terminal_modal {
                let overlay_w = (term_size.width * 90 / 100).max(20);
                let overlay_h = (chat_area_h * 85 / 100).max(10);
                let inner_cols = overlay_w.saturating_sub(2);
                let inner_rows = overlay_h.saturating_sub(2);
                modal.resize(inner_cols, inner_rows);
            }

            if self.trust_prompt.is_none()
                && self.approval_prompt.is_none()
                && self.plan_review.is_none()
                && self.planner_question.is_none()
                && self.session_picker.is_none()
                && self.bg_picker.is_none()
                && self.worker_picker.is_none()
                && self.subagent_picker.is_none()
                && self.rewind_picker.is_none()
                && self.memory_viewer.is_none()
                && self.tool_picker.is_none()
                && self.event_picker.is_none()
                && !self.terminal_visible
            {
                let (msgs, committed_idx, streaming, vscroll) = match self.active_view {
                    ActiveView::Main => (
                        &self.messages,
                        &mut self.committed_msg_idx,
                        self.streaming_text.as_str(),
                        &mut self.viewport_scroll,
                    ),
                    ActiveView::EventAgent => (
                        &self.event_messages,
                        &mut self.event_committed_msg_idx,
                        self.event_streaming_text.as_str(),
                        &mut self.event_viewport_scroll,
                    ),
                };
                let render_width = term_size.width.saturating_sub(1);
                helpers::scroll_overflow(
                    msgs,
                    committed_idx,
                    vscroll,
                    &mut terminal,
                    chat_area_h,
                    render_width,
                    streaming,
                )?;
            }

            // Pre-compute palette state for this frame
            let filtered_cmds = helpers::filtered_commands(&self.input);
            let bg_picker_active = self.bg_picker.is_some();
            let worker_picker_active = self.worker_picker.is_some();
            let subagent_picker_active = self.subagent_picker.is_some();
            let rewind_picker_active = self.rewind_picker.is_some();
            let cmd_palette_active = !filtered_cmds.is_empty()
                && !self.is_streaming
                && self.session_picker.is_none()
                && self.model_picker.is_none()
                && self.reasoning_picker.is_none()
                && !bg_picker_active
                && !worker_picker_active
                && !subagent_picker_active
                && !rewind_picker_active;
            let model_picker_active = self.model_picker.is_some() && self.session_picker.is_none();
            let reasoning_picker_active = self.reasoning_picker.is_some();
            let palette_visible =
                cmd_palette_active || model_picker_active || reasoning_picker_active;
            let palette_h = if reasoning_picker_active {
                self.reasoning_picker.as_ref().unwrap().levels.len() as u16 + 2
            } else if model_picker_active {
                self.model_picker.as_ref().unwrap().models.len() as u16 + 2
            } else if cmd_palette_active {
                filtered_cmds.len() as u16 + 1
            } else {
                0
            };
            let palette_idx = self
                .cmd_palette_idx
                .min(filtered_cmds.len().saturating_sub(1));

            // Force full redraw if needed (clears ratatui's diff buffer)
            if self.force_redraw {
                terminal.clear()?;
                self.force_redraw = false;
            }

            let input_h = cur_input_h;

            // Draw UI
            terminal.draw(|frame| {
                self.render_frame(
                    frame,
                    palette_visible,
                    palette_h,
                    palette_idx,
                    &filtered_cmds,
                    input_h,
                    task_panel_h,
                );
            })?;

            // Handle events
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    match event {
                        AppEvent::Key(key) => {
                            if self.handle_key_event(key, &agent_tx, &event_agent_tx).await {
                                continue;
                            }
                        }
                        AppEvent::Paste(text) => {
                            // Forward paste to terminal modal if visible
                            if self.terminal_visible
                                && let Some(ref mut modal) = self.terminal_modal
                            {
                                modal.send_paste(&text);
                            } else if !self.is_streaming
                                && self.session_picker.is_none()
                                && self.model_picker.is_none()
                                && self.reasoning_picker.is_none()
                            {
                                // Aggressive sanitization for pasted text:
                                // - Remove all ANSI escape sequences (ESC [ ... m)
                                // - Remove bracketed paste markers (ESC[200~, ESC[201~)
                                // - Convert tabs to spaces (preserve visual layout)
                                // - Normalize \r and \r\n to \n
                                // Step 1: Remove ANSI escape sequences: ESC followed by [ and ending with letter
                                let mut cleaned = String::with_capacity(text.len());
                                let mut chars = text.chars().peekable();
                                while let Some(ch) = chars.next() {
                                    if ch == '\x1b' {
                                        // Skip escape sequence
                                        if chars.peek() == Some(&'[') {
                                            chars.next(); // consume [
                                            // Consume until we hit a letter (end of CSI sequence)
                                            while let Some(&seq_ch) = chars.peek() {
                                                chars.next();
                                                if seq_ch.is_ascii_alphabetic() || seq_ch == '~' {
                                                    break;
                                                }
                                            }
                                        } else {
                                            // Skip next char after ESC if not [
                                            chars.next();
                                        }
                                    } else {
                                        cleaned.push(ch);
                                    }
                                }

                                // Step 2: Normalize whitespace and remove remaining control chars
                                let sanitized: String = cleaned
                                    .chars()
                                    .filter_map(|c| match c {
                                        '\t' => Some(' '),
                                        '\r' => None,
                                        '\n' => Some('\n'),
                                        c if c.is_control() && c != '\n' => None,
                                        c => Some(c),
                                    })
                                    .collect();

                                let char_count = sanitized.chars().count();
                                let start = self.cursor_pos;
                                self.input.insert_str(start, &sanitized);
                                let end = start + sanitized.len();
                                self.cursor_pos = end;
                                // Short pastes: inline. Long pastes: collapsed block.
                                if char_count > 200 {
                                    let label = format!("[Pasted Content {char_count} chars]");
                                    self.paste_info = Some((start, end, label));
                                }
                                self.esc_pending = false;
                                self.cmd_palette_idx = 0;
                            }
                        }
                        AppEvent::Resize(_, _) => {
                            // Resize terminal modal if active
                            if let Some(ref mut modal) = self.terminal_modal {
                                let term_size = crossterm::terminal::size().unwrap_or((80, 24));
                                let overlay_w = (term_size.0 * 90 / 100).max(20);
                                let overlay_h = (term_size.1 * 85 / 100).max(10);
                                let inner_cols = overlay_w.saturating_sub(2);
                                let inner_rows = overlay_h.saturating_sub(2);
                                modal.resize(inner_cols, inner_rows);
                                self.force_redraw = true;
                            }
                        }
                        AppEvent::Mouse(mouse) => {
                            // Only handle mouse events when terminal modal is visible
                            if self.terminal_visible
                                && let Some(ref mut modal) = self.terminal_modal
                                && let Some(inner) = self.terminal_overlay_inner
                            {
                                use crossterm::event::{MouseEventKind, MouseButton};
                                let col = mouse.column;
                                let row = mouse.row;
                                // Check if mouse is within the terminal overlay inner area
                                let inside = col >= inner.x
                                    && col < inner.x + inner.width
                                    && row >= inner.y
                                    && row < inner.y + inner.height;
                                if inside {
                                    let rel_col = col - inner.x;
                                    let rel_row = row - inner.y;
                                    match mouse.kind {
                                        MouseEventKind::Down(MouseButton::Left) => {
                                            modal.start_selection(rel_row, rel_col);
                                        }
                                        MouseEventKind::Drag(MouseButton::Left) => {
                                            modal.update_selection(rel_row, rel_col);
                                        }
                                        MouseEventKind::Up(MouseButton::Left) => {
                                            modal.update_selection(rel_row, rel_col);
                                            // Copy selected text to clipboard
                                            if let Some(text) = modal.extract_selected_text() {
                                                Self::copy_to_clipboard(&text);
                                            }
                                        }
                                        MouseEventKind::ScrollUp => {
                                            modal.screen_mut().scroll_up(3);
                                        }
                                        MouseEventKind::ScrollDown => {
                                            modal.screen_mut().scroll_down(3);
                                        }
                                        _ => {}
                                    }
                                } else {
                                    // Click outside modal clears selection
                                    modal.clear_selection();
                                }
                            }
                        }
                        AppEvent::Tick => {
                            // Process terminal modal output (even when minimized)
                            if let Some(ref mut modal) = self.terminal_modal {
                                modal.process_output();
                            }
                            // Auto-refresh bg_picker log view
                            if let Some(ref mut picker) = self.bg_picker {
                                if self.daemon_tx.is_some() {
                                    // Daemon mode: request fresh log from daemon (throttled)
                                    if picker.view == BgPickerView::Log
                                        && picker.log_pid > 0
                                        && picker.last_log_refresh_elapsed()
                                        && let Some(ref daemon) = self.daemon_tx
                                    {
                                        let _ = daemon.send(
                                            &crate::ws_client::OutMessage::GetBgProcessLog {
                                                pid: picker.log_pid,
                                            },
                                        );
                                    }
                                } else {
                                    picker.refresh_log();
                                }
                            }
                            // Expire old toast notifications (5 seconds)
                            self.toast_notifications
                                .retain(|(_, created)| created.elapsed().as_secs() < 5);
                            // Poll worker daemon notifications
                            for notif in self.notification_watcher.poll().await {
                                let icon = if notif.status == "success" { "✓" } else { "✗" };
                                let msg = format!("{icon} Worker '{}': {}", notif.worker_name, notif.summary);
                                self.toast_notifications.push((msg, Instant::now()));
                            }
                            // Poll pending.jsonl for new system events (every 3 seconds)
                            if self.last_event_poll.elapsed().as_secs() >= 3 {
                                self.last_event_poll = Instant::now();
                                let has_critical = self.poll_pending_events_to_event_agent();
                                if has_critical && self.event_auto_mode {
                                    // Auto mode: trigger event agent directly
                                    self.trigger_event_agent_auto_turn(event_agent_tx.clone()).await;
                                } else if has_critical && !self.event_auto_mode && self.event_picker.is_none() {
                                    // Manual mode: auto-open picker for critical events
                                    let events: Vec<_> = self.event_buffer.drain(..).collect();
                                    if !events.is_empty() {
                                        self.event_picker = Some(EventPicker::new_picker(events));
                                        self.event_agent_has_unread = true;
                                        self.force_redraw = true;
                                    }
                                }
                            }
                        }
                    }
                }
                Some(stream_event) = agent_rx.recv() => {
                    self.handle_stream_event(stream_event);
                    // Drain all ready agent events before re-rendering so rapid
                    // streaming doesn't starve terminal input / Tick processing.
                    while let Ok(ev) = agent_rx.try_recv() {
                        self.handle_stream_event(ev);
                    }
                    // Keep terminal responsive during agent streaming
                    if let Some(ref mut modal) = self.terminal_modal {
                        modal.process_output();
                    }
                }
                Some(event_stream_event) = event_agent_rx.recv() => {
                    self.handle_event_agent_stream_event(event_stream_event);
                    while let Ok(ev) = event_agent_rx.try_recv() {
                        self.handle_event_agent_stream_event(ev);
                    }
                }
                Some(update) = subagent_update_rx.recv() => {
                    self.handle_subagent_update(update);
                    while let Ok(ev) = subagent_update_rx.try_recv() {
                        self.handle_subagent_update(ev);
                    }
                }
                // Daemon WebSocket events (when in daemon mode)
                Some(daemon_ev) = async {
                    match self.daemon_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.handle_daemon_event(daemon_ev);
                    // Drain ready events (collect first to avoid borrow conflict)
                    let mut drained = Vec::new();
                    if let Some(ref mut rx) = self.daemon_rx {
                        while let Ok(ev) = rx.try_recv() {
                            drained.push(ev);
                        }
                    }
                    for ev in drained {
                        self.handle_daemon_event(ev);
                    }
                }
            }

            // Auto-submit queued messages after daemon turn completes
            if self.pending_queue_submit {
                self.pending_queue_submit = false;
                self.submit_message(agent_tx.clone()).await;
            }

            // Auto-refresh task panel after task tool events
            if self.task_panel_needs_refresh {
                self.task_panel_needs_refresh = false;
                let cwd = std::env::current_dir().unwrap_or_default();
                self.task_panel_entries = lukan_tools::tasks::read_all_tasks(&cwd)
                    .await
                    .into_iter()
                    .filter(|t| t.status != lukan_tools::tasks::TaskStatus::Done)
                    .collect();
                if !self.task_panel_visible && !self.task_panel_entries.is_empty() {
                    self.task_panel_visible = true; // Auto-show when tasks first appear
                }
            }

            // Recover agent after turn completes (when no longer streaming)
            if !self.is_streaming
                && let Some(mut rx) = self.agent_return_rx.take()
            {
                match rx.try_recv() {
                    Ok(mut agent) => {
                        // Sync permission mode in case it was changed via
                        // Shift+Tab while the agent was running its turn
                        agent.set_permission_mode(self.permission_mode.clone());
                        agent.set_disabled_tools(self.disabled_tools.clone());
                        // Sync token counters from agent state (important after /compact,
                        // which resets tokens without emitting StreamEvent::Usage to UI).
                        self.input_tokens = agent.input_tokens();
                        self.output_tokens = agent.output_tokens();
                        self.context_size = agent.last_context_size();
                        self.session_id = Some(agent.session_id().to_string());
                        self.agent = Some(agent);

                        // Auto-submit any remaining queued messages as a new turn
                        let remaining: Vec<String> =
                            self.queued_messages.lock().unwrap().drain(..).collect();
                        if !remaining.is_empty() {
                            self.input = remaining.join("\n");
                            self.cursor_pos = self.input.len();
                            self.submit_message(agent_tx.clone()).await;
                        }
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                        // Not ready yet, put it back
                        self.agent_return_rx = Some(rx);
                    }
                    Err(_) => {} // Sender dropped, agent lost
                }
            }

            // Recover event agent after its turn completes
            if !self.event_is_streaming
                && let Some(mut rx) = self.event_agent_return_rx.take()
            {
                match rx.try_recv() {
                    Ok(mut agent) => {
                        agent.set_disabled_tools(self.disabled_tools.clone());
                        self.event_agent = Some(agent);
                        // Flush any events that arrived while event agent was busy
                        self.flush_event_buffer_to_event_agent();
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                        self.event_agent_return_rx = Some(rx);
                    }
                    Err(_) => {} // Sender dropped, event agent lost
                }
            }

            if self.should_quit {
                break;
            }
        }

        // Save session before exiting (only if not stale, to avoid
        // overwriting newer data written by another client)
        if let Some(ref mut agent) = self.agent
            && let Err(e) = agent.save_session_if_not_stale().await
        {
            tracing::warn!(error = %e, "Failed to save session on exit");
        }

        // Close terminal modal if open
        if let Some(modal) = self.terminal_modal.take() {
            modal.close();
        }

        // Clean up background processes before exiting
        lukan_tools::bg_processes::cleanup_all();

        // Disable mouse capture if it was enabled
        if self.mouse_capture_enabled {
            let _ = stdout().execute(crossterm::event::DisableMouseCapture);
        }
        stdout().execute(crossterm::event::DisableBracketedPaste)?;
        disable_raw_mode()?;
        println!(); // new line so the shell prompt starts clean
        Ok(())
    }
}
