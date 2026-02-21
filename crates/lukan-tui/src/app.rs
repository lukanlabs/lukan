use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    event::{DisableMouseCapture, EnableMouseCapture, KeyCode, MouseEventKind},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
};
use std::collections::HashMap;
use std::io::stdout;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::error;

use lukan_agent::{AgentConfig, AgentLoop, SessionManager};
use lukan_core::config::{ConfigManager, CredentialsManager, ProviderName, ResolvedConfig};
use lukan_core::models::events::StreamEvent;
use lukan_core::models::sessions::SessionSummary;
use lukan_providers::{Provider, SystemPrompt, create_provider};
use lukan_tools::create_default_registry;

use chrono::Utc;

use crate::event::{AppEvent, is_quit, spawn_event_reader};
use crate::widgets::chat::{ChatMessage, ChatWidget, rendered_line_count};
use crate::widgets::input::InputWidget;
use crate::widgets::status_bar::StatusBarWidget;

/// Application state
pub struct App {
    messages: Vec<ChatMessage>,
    input: String,
    cursor_pos: usize,
    streaming_text: String,
    is_streaming: bool,
    scroll_offset: u16,
    auto_scroll: bool,
    input_tokens: u64,
    output_tokens: u64,
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
}

/// Interactive model picker state
struct ModelPicker {
    models: Vec<String>,
    selected: usize,
    current: String,
}

/// Interactive session picker state
struct SessionPicker {
    sessions: Vec<SessionSummary>,
    selected: usize,
    current_id: Option<String>,
}

impl App {
    pub fn new(provider: Box<dyn Provider>, config: ResolvedConfig) -> Self {
        let provider = Arc::from(provider);

        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            streaming_text: String::new(),
            is_streaming: false,
            scroll_offset: 0,
            auto_scroll: true,
            input_tokens: 0,
            output_tokens: 0,
            provider,
            config,
            should_quit: false,
            model_picker: None,
            session_picker: None,
            agent: None,
            agent_return_rx: None,
            active_tool: None,
            session_id: None,
        }
    }

    /// Create or get the agent loop, initializing it on first use (async)
    async fn ensure_agent(&mut self) -> &mut AgentLoop {
        if self.agent.is_none() {
            let agent = self.create_agent().await;
            self.session_id = Some(agent.session_id().to_string());
            self.agent = Some(agent);
        }

        self.agent.as_mut().unwrap()
    }

    /// Create a new AgentLoop with a fresh session
    async fn create_agent(&self) -> AgentLoop {
        let system_prompt =
            SystemPrompt::Text(include_str!("../../../prompts/base.txt").to_string());

        let cwd = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());

        let config = AgentConfig {
            provider: Arc::clone(&self.provider),
            tools: create_default_registry(),
            system_prompt,
            cwd,
            provider_name: self.config.config.provider.to_string(),
            model_name: self.config.effective_model(),
        };

        match AgentLoop::new(config).await {
            Ok(agent) => agent,
            Err(e) => {
                // Fallback: if session creation fails, log error and panic
                // This shouldn't happen in normal operation
                panic!("Failed to create agent session: {e}");
            }
        }
    }

    pub async fn run(mut self) -> Result<()> {
        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        stdout().execute(EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AppEvent>();
        spawn_event_reader(event_tx);

        let (agent_tx, mut agent_rx) = mpsc::channel::<StreamEvent>(256);

        // Welcome banner
        self.messages.push(ChatMessage::new(
            "banner",
            build_welcome_banner(self.provider.name(), &self.config.effective_model()),
        ));

        loop {
            // Auto-scroll to bottom when following new content
            if self.auto_scroll {
                let size = terminal.size()?;
                // viewport = total height minus input (3 rows) and status bar (1 row)
                let viewport_h = size.height.saturating_sub(4);
                let total = rendered_line_count(&self.messages, &self.streaming_text);
                self.scroll_offset = total.saturating_sub(viewport_h);
            }

            // Draw UI
            terminal.draw(|frame| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Min(1),    // Chat area
                        Constraint::Length(3), // Input
                        Constraint::Length(1), // Status bar
                    ])
                    .split(frame.area());

                // Chat (or picker overlay)
                if let Some(ref picker) = self.session_picker {
                    let widget = SessionPickerWidget::new(picker);
                    frame.render_widget(widget, chunks[0]);
                } else if let Some(ref picker) = self.model_picker {
                    let widget = ModelPickerWidget::new(picker);
                    frame.render_widget(widget, chunks[0]);
                } else {
                    let chat =
                        ChatWidget::new(&self.messages, &self.streaming_text, self.scroll_offset);
                    frame.render_widget(chat, chunks[0]);
                }

                // Input
                let input_widget = if self.session_picker.is_some() || self.model_picker.is_some() {
                    InputWidget::new("↑↓ navigate · Enter select · ESC close", 0, false)
                } else {
                    InputWidget::new(&self.input, self.cursor_pos, !self.is_streaming)
                };
                frame.render_widget(input_widget, chunks[1]);

                // Status bar
                let effective_model = self.config.effective_model();
                let status = StatusBarWidget::new(
                    self.provider.name(),
                    &effective_model,
                    self.input_tokens,
                    self.output_tokens,
                    self.is_streaming,
                );
                frame.render_widget(status, chunks[2]);

                // Set cursor position only when not in picker and not streaming
                if self.session_picker.is_none()
                    && self.model_picker.is_none()
                    && !self.is_streaming
                {
                    let cursor_x = chunks[1].x + 1 + self.cursor_pos as u16;
                    let cursor_y = chunks[1].y + 1;
                    frame.set_cursor_position((cursor_x, cursor_y));
                }
            })?;

            // Handle events
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    match event {
                        AppEvent::Key(key) => {
                            if self.session_picker.is_some() {
                                // Session picker mode
                                match key.code {
                                    KeyCode::Up => {
                                        if let Some(ref mut picker) = self.session_picker
                                            && picker.selected > 0
                                        {
                                            picker.selected -= 1;
                                        }
                                    }
                                    KeyCode::Down => {
                                        if let Some(ref mut picker) = self.session_picker
                                            && picker.selected + 1
                                                < picker.sessions.len()
                                        {
                                            picker.selected += 1;
                                        }
                                    }
                                    KeyCode::Enter => {
                                        let idx =
                                            self.session_picker.as_ref().unwrap().selected;
                                        self.load_selected_session(idx).await;
                                        self.session_picker = None;
                                    }
                                    KeyCode::Esc => {
                                        self.session_picker = None;
                                    }
                                    _ => {}
                                }
                            } else if self.model_picker.is_some() {
                                // Model picker mode
                                match key.code {
                                    KeyCode::Up => {
                                        if let Some(ref mut picker) = self.model_picker
                                            && picker.selected > 0
                                        {
                                            picker.selected -= 1;
                                        }
                                    }
                                    KeyCode::Down => {
                                        if let Some(ref mut picker) = self.model_picker
                                            && picker.selected + 1 < picker.models.len()
                                        {
                                            picker.selected += 1;
                                        }
                                    }
                                    KeyCode::Enter => {
                                        let idx = self.model_picker.as_ref().unwrap().selected;
                                        self.select_model(idx).await;
                                        self.model_picker = None;
                                    }
                                    KeyCode::Esc => {
                                        self.model_picker = None;
                                    }
                                    _ => {}
                                }
                            } else if is_quit(&key) {
                                self.should_quit = true;
                            } else if key.code == KeyCode::PageUp {
                                // Scroll up (works even while streaming)
                                self.auto_scroll = false;
                                let half = terminal.size().map(|s| s.height / 2).unwrap_or(10);
                                self.scroll_offset = self.scroll_offset.saturating_sub(half);
                            } else if key.code == KeyCode::PageDown {
                                // Scroll down (works even while streaming)
                                let size = terminal.size().unwrap_or_default();
                                let viewport_h = size.height.saturating_sub(4);
                                let total = rendered_line_count(&self.messages, &self.streaming_text);
                                let max_scroll = total.saturating_sub(viewport_h);
                                self.scroll_offset = (self.scroll_offset + size.height / 2).min(max_scroll);
                                if self.scroll_offset >= max_scroll {
                                    self.auto_scroll = true;
                                }
                            } else if !self.is_streaming {
                                match key.code {
                                    KeyCode::Enter => {
                                        if !self.input.trim().is_empty() {
                                            self.submit_message(agent_tx.clone()).await;
                                        }
                                    }
                                    KeyCode::Char(c) => {
                                        self.input.insert(self.cursor_pos, c);
                                        self.cursor_pos += 1;
                                    }
                                    KeyCode::Backspace => {
                                        if self.cursor_pos > 0 {
                                            self.cursor_pos -= 1;
                                            self.input.remove(self.cursor_pos);
                                        }
                                    }
                                    KeyCode::Left => {
                                        if self.cursor_pos > 0 {
                                            self.cursor_pos -= 1;
                                        }
                                    }
                                    KeyCode::Right => {
                                        if self.cursor_pos < self.input.len() {
                                            self.cursor_pos += 1;
                                        }
                                    }
                                    KeyCode::Home => self.cursor_pos = 0,
                                    KeyCode::End => self.cursor_pos = self.input.len(),
                                    _ => {}
                                }
                            }
                        }
                        AppEvent::Mouse(mouse) => {
                            match mouse.kind {
                                MouseEventKind::ScrollUp => {
                                    self.auto_scroll = false;
                                    self.scroll_offset = self.scroll_offset.saturating_sub(3);
                                }
                                MouseEventKind::ScrollDown => {
                                    let size = terminal.size().unwrap_or_default();
                                    let viewport_h = size.height.saturating_sub(4);
                                    let total = rendered_line_count(&self.messages, &self.streaming_text);
                                    let max_scroll = total.saturating_sub(viewport_h);
                                    self.scroll_offset = (self.scroll_offset + 3).min(max_scroll);
                                    if self.scroll_offset >= max_scroll {
                                        self.auto_scroll = true;
                                    }
                                }
                                _ => {}
                            }
                        }
                        AppEvent::Resize(_, _) => {}
                        AppEvent::Tick => {}
                    }
                }
                Some(stream_event) = agent_rx.recv() => {
                    self.handle_stream_event(stream_event);
                }
            }

            // Recover agent after turn completes (when no longer streaming)
            if !self.is_streaming
                && let Some(mut rx) = self.agent_return_rx.take()
            {
                match rx.try_recv() {
                    Ok(agent) => {
                        self.session_id = Some(agent.session_id().to_string());
                        self.agent = Some(agent);
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                        // Not ready yet, put it back
                        self.agent_return_rx = Some(rx);
                    }
                    Err(_) => {} // Sender dropped, agent lost
                }
            }

            if self.should_quit {
                break;
            }
        }

        stdout().execute(DisableMouseCapture)?;
        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;
        Ok(())
    }

    async fn submit_message(&mut self, agent_tx: mpsc::Sender<StreamEvent>) {
        let text = self.input.trim().to_string();
        self.input.clear();
        self.cursor_pos = 0;

        // Handle /chats command
        if text == "/chats" {
            self.open_session_picker().await;
            return;
        }

        // Handle /model command
        if text == "/model" || text.starts_with("/model ") {
            self.open_model_picker().await;
            return;
        }

        // Handle /clear
        if text == "/clear" {
            self.messages.clear();
            self.input_tokens = 0;
            self.output_tokens = 0;
            // Reset agent — a new session will be created on next message
            self.agent = None;
            self.session_id = None;
            return;
        }

        // Regular message
        self.messages.push(ChatMessage::new("user", text.clone()));

        self.is_streaming = true;
        self.streaming_text.clear();
        self.active_tool = None;
        self.auto_scroll = true;

        // Ensure agent exists (create new session if needed) and run the turn
        // We need to take the agent out to avoid borrow issues with self
        let agent = match self.agent.take() {
            Some(a) => a,
            None => self.create_agent().await,
        };

        self.session_id = Some(agent.session_id().to_string());

        // Oneshot channel to get the agent back after the turn
        let (return_tx, return_rx) = tokio::sync::oneshot::channel::<AgentLoop>();
        self.agent_return_rx = Some(return_rx);

        let mut agent = agent;
        tokio::spawn(async move {
            if let Err(e) = agent.run_turn(&text, agent_tx.clone()).await {
                error!("Agent loop error: {e}");
                agent_tx
                    .send(StreamEvent::Error {
                        error: e.to_string(),
                    })
                    .await
                    .ok();
            }

            // Return the agent so history persists
            let _ = return_tx.send(agent);
        });
    }

    /// Open the interactive session picker
    async fn open_session_picker(&mut self) {
        let sessions = match SessionManager::list().await {
            Ok(s) => s,
            Err(e) => {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Failed to load sessions: {e}"),
                ));
                return;
            }
        };

        if sessions.is_empty() {
            self.messages
                .push(ChatMessage::new("system", "No saved sessions."));
            return;
        }

        // Pre-select the current session
        let current_id = self.session_id.clone();
        let selected = current_id
            .as_ref()
            .and_then(|id| sessions.iter().position(|s| s.id == *id))
            .unwrap_or(0);

        self.session_picker = Some(SessionPicker {
            sessions,
            selected,
            current_id,
        });
    }

    /// Load the selected session from the picker
    async fn load_selected_session(&mut self, idx: usize) {
        let session_id = {
            let picker = self.session_picker.as_ref().unwrap();
            picker.sessions[idx].id.clone()
        };

        // Don't reload the current session
        if self.session_id.as_deref() == Some(&session_id) {
            self.messages
                .push(ChatMessage::new("system", "Already in this session."));
            return;
        }

        let system_prompt =
            SystemPrompt::Text(include_str!("../../../prompts/base.txt").to_string());
        let cwd = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());

        let config = AgentConfig {
            provider: Arc::clone(&self.provider),
            tools: create_default_registry(),
            system_prompt,
            cwd,
            provider_name: self.config.config.provider.to_string(),
            model_name: self.config.effective_model(),
        };

        match AgentLoop::load_session(config, &session_id).await {
            Ok(agent) => {
                // Rebuild UI messages from the loaded session
                self.messages.clear();

                // Reconstruct chat messages from agent history
                let session = SessionManager::load(&session_id).await.ok().flatten();
                if let Some(session) = session {
                    use lukan_core::models::messages::{ContentBlock, MessageContent, Role};

                    // First pass: collect tool results by tool_use_id
                    let mut tool_results: HashMap<String, (String, bool, Option<String>)> =
                        HashMap::new();
                    for msg in &session.messages {
                        if let MessageContent::Blocks(blocks) = &msg.content {
                            for block in blocks {
                                if let ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                    is_error,
                                    diff,
                                    ..
                                } = block
                                {
                                    tool_results.insert(
                                        tool_use_id.clone(),
                                        (content.clone(), is_error.unwrap_or(false), diff.clone()),
                                    );
                                }
                            }
                        }
                    }

                    // Second pass: reconstruct UI messages
                    for msg in &session.messages {
                        match msg.role {
                            Role::User => {
                                // Only show user messages that have text (skip tool-result-only messages)
                                let text = match &msg.content {
                                    MessageContent::Text(s) => Some(s.clone()),
                                    MessageContent::Blocks(blocks) => {
                                        let texts: Vec<&str> = blocks
                                            .iter()
                                            .filter_map(|b| {
                                                if let ContentBlock::Text { text } = b {
                                                    Some(text.as_str())
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect();
                                        if texts.is_empty() {
                                            None
                                        } else {
                                            Some(texts.join("\n"))
                                        }
                                    }
                                };
                                if let Some(text) = text
                                    && !text.is_empty()
                                {
                                    self.messages.push(ChatMessage::new("user", text));
                                }
                            }
                            Role::Assistant => match &msg.content {
                                MessageContent::Text(text) => {
                                    if !text.is_empty() {
                                        self.messages
                                            .push(ChatMessage::new("assistant", text.clone()));
                                    }
                                }
                                MessageContent::Blocks(blocks) => {
                                    // Collect text blocks
                                    let text: String = blocks
                                        .iter()
                                        .filter_map(|b| {
                                            if let ContentBlock::Text { text } = b {
                                                Some(text.as_str())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n");

                                    if !text.is_empty() {
                                        self.messages.push(ChatMessage::new("assistant", text));
                                    }

                                    // Render tool uses with their results
                                    for block in blocks {
                                        if let ContentBlock::ToolUse { id, name, input } = block {
                                            let summary = summarize_tool_input(name, input);
                                            self.messages.push(ChatMessage::new(
                                                "tool_call",
                                                format!("● {name}({summary})"),
                                            ));

                                            if let Some((content, is_error, diff)) =
                                                tool_results.get(id)
                                            {
                                                let formatted =
                                                    format_tool_result(content, *is_error);
                                                self.messages.push(ChatMessage::with_diff(
                                                    "tool_result",
                                                    formatted,
                                                    diff.clone(),
                                                ));
                                            }
                                        }
                                    }
                                }
                            },
                            _ => {}
                        }
                    }
                }

                self.input_tokens = agent.input_tokens();
                self.output_tokens = agent.output_tokens();
                self.session_id = Some(agent.session_id().to_string());
                self.agent = Some(agent);
                self.auto_scroll = true;

                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Loaded session {session_id}"),
                ));
            }
            Err(e) => {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Failed to load session: {e}"),
                ));
            }
        }
    }

    /// Open the interactive model picker
    async fn open_model_picker(&mut self) {
        let models = match ConfigManager::get_models().await {
            Ok(m) => m,
            Err(e) => {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Failed to load models: {e}"),
                ));
                return;
            }
        };

        if models.is_empty() {
            self.messages.push(ChatMessage::new(
                "system",
                "No models available. Run 'lukan setup' to configure providers.",
            ));
            return;
        }

        let current = format!(
            "{}:{}",
            self.config.config.provider,
            self.config.effective_model()
        );

        // Pre-select the current model
        let selected = models.iter().position(|m| *m == current).unwrap_or(0);

        self.model_picker = Some(ModelPicker {
            models,
            selected,
            current,
        });
    }

    /// Switch to the selected model from the picker
    async fn select_model(&mut self, idx: usize) {
        let picker = self.model_picker.as_ref().unwrap();
        let entry = &picker.models[idx];

        let Some((provider_str, model_name)) = entry.split_once(':') else {
            self.messages.push(ChatMessage::new(
                "system",
                format!("Invalid model format: {entry}"),
            ));
            return;
        };

        let provider_name: ProviderName =
            match serde_json::from_value(serde_json::Value::String(provider_str.to_string())) {
                Ok(p) => p,
                Err(_) => {
                    self.messages.push(ChatMessage::new(
                        "system",
                        format!("Unknown provider: {provider_str}"),
                    ));
                    return;
                }
            };

        let credentials = match CredentialsManager::load().await {
            Ok(c) => c,
            Err(e) => {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Failed to load credentials: {e}"),
                ));
                return;
            }
        };

        let mut new_config = self.config.clone();
        new_config.config.provider = provider_name;
        new_config.config.model = Some(model_name.to_string());
        new_config.credentials = credentials;

        match create_provider(&new_config) {
            Ok(new_provider) => {
                self.provider = Arc::from(new_provider);
                self.config = new_config;
                // Reset agent so it picks up the new provider (new session)
                self.agent = None;
                self.session_id = None;
                self.messages
                    .push(ChatMessage::new("system", format!("Switched to {entry}")));
            }
            Err(e) => {
                self.messages
                    .push(ChatMessage::new("system", format!("Failed to switch: {e}")));
            }
        }
    }

    fn handle_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::MessageStart => {
                self.streaming_text.clear();
                self.active_tool = None;
            }
            StreamEvent::TextDelta { text } => {
                self.streaming_text.push_str(&text);
            }
            StreamEvent::ThinkingDelta { text } => {
                self.streaming_text.push_str(&text);
            }
            StreamEvent::ToolUseStart { name, .. } => {
                // Flush current text as a message before tool call
                self.active_tool = Some(name.clone());
                if !self.streaming_text.is_empty() {
                    let content = std::mem::take(&mut self.streaming_text);
                    self.messages.push(ChatMessage::new("assistant", content));
                }
            }
            StreamEvent::ToolUseEnd { name, input, .. } => {
                // ● ToolName(input summary)
                let summary = summarize_tool_input(&name, &input);
                self.messages.push(ChatMessage::new(
                    "tool_call",
                    format!("● {name}({summary})"),
                ));
            }
            StreamEvent::ToolProgress { name, content, .. } => {
                self.active_tool = Some(name);
                self.messages
                    .push(ChatMessage::new("tool_result", format!("  ⎿  {content}")));
            }
            StreamEvent::ToolResult {
                content,
                is_error,
                diff,
                ..
            } => {
                self.active_tool = None;
                // Build summary line
                let formatted = format_tool_result(&content, is_error.unwrap_or(false));
                self.messages
                    .push(ChatMessage::with_diff("tool_result", formatted, diff));
            }
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
                ..
            } => {
                self.input_tokens += input_tokens;
                self.output_tokens += output_tokens;
            }
            StreamEvent::MessageEnd { .. } => {
                if !self.streaming_text.is_empty() {
                    let content = std::mem::take(&mut self.streaming_text);
                    self.messages.push(ChatMessage::new("assistant", content));
                }
                self.is_streaming = false;
                self.active_tool = None;
            }
            StreamEvent::Error { error } => {
                self.messages
                    .push(ChatMessage::new("assistant", format!("Error: {error}")));
                self.is_streaming = false;
            }
            _ => {}
        }
    }
}

// ── Tool Input Summary ────────────────────────────────────────────────────

/// Produce a human-readable one-liner for the tool call input
fn summarize_tool_input(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Bash" => input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("(no command)")
            .to_string(),
        "ReadFile" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let mut s = path.to_string();
            if let Some(offset) = input.get("offset").and_then(|v| v.as_u64()) {
                s.push_str(&format!(" (from line {offset})"));
            }
            s
        }
        "WriteFile" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let len = input
                .get("content")
                .and_then(|v| v.as_str())
                .map(|c| c.len())
                .unwrap_or(0);
            format!("{path} ({len} bytes)")
        }
        "EditFile" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let replace_all = input
                .get("replace_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if replace_all {
                format!("{path} (replace all)")
            } else {
                path.to_string()
            }
        }
        "Grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            format!("{pattern} in {path}")
        }
        "Glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        "WebFetch" => input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        _ => {
            // Fallback: compact JSON
            let s = serde_json::to_string(input).unwrap_or_default();
            if s.len() > 200 {
                format!("{}...", &s[..200])
            } else {
                s
            }
        }
    }
}

// ── Tool Result Formatting ────────────────────────────────────────────────

/// Format tool result with ⎿ prefix on each line, like Claude Code
fn format_tool_result(content: &str, is_error: bool) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return "  ⎿  (no output)".to_string();
    }

    let prefix = if is_error { "  ⎿  ✗ " } else { "  ⎿  " };

    let mut result = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            result.push_str(&format!("{prefix}{line}"));
        } else {
            result.push_str(&format!("\n     {line}"));
        }
    }
    result
}

// ── Welcome Banner ────────────────────────────────────────────────────────

fn build_welcome_banner(provider: &str, model: &str) -> String {
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".to_string());

    format!(
        "\
 ██╗     ██╗   ██╗██╗  ██╗ █████╗ ███╗   ██╗
 ██║     ██║   ██║██║ ██╔╝██╔══██╗████╗  ██║   AI Agent CLI
 ██║     ██║   ██║█████╔╝ ███████║██╔██╗ ██║   {provider} > {model}
 ██║     ██║   ██║██╔═██╗ ██╔══██║██║╚██╗██║
 ███████╗╚██████╔╝██║  ██╗██║  ██║██║ ╚████║   {cwd}
 ╚══════╝ ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═══╝

 /model  Switch model    /chats  Load session    /clear  Clear chat    Ctrl+C  Quit"
    )
}

// ── Model Picker Widget ──────────────────────────────────────────────────

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

struct ModelPickerWidget<'a> {
    picker: &'a ModelPicker,
}

impl<'a> ModelPickerWidget<'a> {
    fn new(picker: &'a ModelPicker) -> Self {
        Self { picker }
    }
}

impl Widget for ModelPickerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line<'_>> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Select Model",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        for (i, entry) in self.picker.models.iter().enumerate() {
            let is_selected = i == self.picker.selected;
            let is_current = *entry == self.picker.current;

            let pointer = if is_selected { "▸ " } else { "  " };

            // Split provider:model
            let (provider, model) = entry.split_once(':').unwrap_or(("?", entry.as_str()));

            let mut spans = vec![
                Span::styled(
                    pointer,
                    if is_selected {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(
                    provider,
                    if is_selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Yellow)
                    },
                ),
                Span::styled(":", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    model,
                    if is_selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    },
                ),
            ];

            if is_current {
                spans.push(Span::styled(
                    " (current)",
                    Style::default().fg(Color::Green),
                ));
            }

            lines.push(Line::from(spans));
        }

        let block = Block::default().borders(Borders::NONE);
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}

// ── Session Picker Widget ───────────────────────────────────────────────

struct SessionPickerWidget<'a> {
    picker: &'a SessionPicker,
}

impl<'a> SessionPickerWidget<'a> {
    fn new(picker: &'a SessionPicker) -> Self {
        Self { picker }
    }
}

impl Widget for SessionPickerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line<'_>> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Select Session",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        for (i, session) in self.picker.sessions.iter().enumerate() {
            let is_selected = i == self.picker.selected;
            let is_current = self
                .picker
                .current_id
                .as_ref()
                .is_some_and(|id| *id == session.id);

            let pointer = if is_selected { "▸ " } else { "  " };

            let provider_model = match (&session.provider, &session.model) {
                (Some(p), Some(m)) => format!("{p}:{m}"),
                (Some(p), None) => p.clone(),
                _ => "unknown".to_string(),
            };

            let time_ago = format_time_ago(session.updated_at);
            let msg_count = session.message_count;

            let mut spans = vec![
                Span::styled(
                    pointer,
                    if is_selected {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(
                    format!("[{}]", session.id),
                    if is_selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Yellow)
                    },
                ),
                Span::styled(" ", Style::default()),
                Span::styled(
                    provider_model,
                    if is_selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    },
                ),
                Span::styled(
                    format!(" · {msg_count} msgs · {time_ago}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ];

            if is_current {
                spans.push(Span::styled(
                    " (current)",
                    Style::default().fg(Color::Green),
                ));
            }

            lines.push(Line::from(spans));
        }

        let block = Block::default().borders(Borders::NONE);
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}

/// Format a timestamp as a human-readable "time ago" string
fn format_time_ago(dt: chrono::DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(dt);

    let seconds = duration.num_seconds();
    if seconds < 60 {
        return format!("{seconds}s ago");
    }

    let minutes = duration.num_minutes();
    if minutes < 60 {
        return format!("{minutes}m ago");
    }

    let hours = duration.num_hours();
    if hours < 24 {
        return format!("{hours}h ago");
    }

    let days = duration.num_days();
    if days < 30 {
        return format!("{days}d ago");
    }

    let months = days / 30;
    format!("{months}mo ago")
}
