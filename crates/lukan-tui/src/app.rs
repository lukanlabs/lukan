use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    event::KeyCode,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
};
use std::collections::HashMap;
use std::io::{Stdout, stdout};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tracing::error;

use lukan_agent::{AgentConfig, AgentLoop, SessionManager};
use lukan_core::config::{ConfigManager, CredentialsManager, LukanPaths, ProviderName, ResolvedConfig};
use lukan_core::models::events::{StopReason, StreamEvent};
use lukan_core::models::sessions::SessionSummary;
use lukan_providers::{Provider, SystemPrompt, create_provider};
use lukan_tools::create_default_registry;

use chrono::Utc;

use crate::event::{AppEvent, is_quit, spawn_event_reader};
use crate::widgets::bg_picker::{BgEntry, BgPicker, BgPickerView, BgPickerWidget};
use crate::widgets::chat::{
    ChatMessage, ChatWidget, build_message_lines, physical_row_count, sanitize_for_display,
};
use crate::widgets::input::{InputWidget, cursor_position, input_height};
use crate::widgets::status_bar::StatusBarWidget;

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
    /// Selected index in the command palette (when typing `/`)
    cmd_palette_idx: usize,
    /// Reasoning effort picker state (shown after selecting a codex model)
    reasoning_picker: Option<ReasoningPicker>,
    /// Force full terminal redraw on next frame (e.g. after closing overlay)
    force_redraw: bool,
    /// First ESC was pressed — show hint and clear on second ESC
    esc_pending: bool,
    /// When set: (start_byte, end_byte, preview_label). Paste block inside self.input.
    paste_info: Option<(usize, usize, String)>,
    /// Background process picker state
    bg_picker: Option<BgPicker>,
    /// Sender to signal Alt+B (send running Bash to background)
    bg_signal_tx: watch::Sender<()>,
    /// Receiver half (cloned into AgentConfig)
    bg_signal_rx: watch::Receiver<()>,
}

/// Interactive model picker state
struct ModelPicker {
    models: Vec<String>,
    selected: usize,
    current: String,
}

/// Reasoning effort picker state
struct ReasoningPicker {
    model_entry: String,
    levels: Vec<(&'static str, &'static str, &'static str)>, // (value, label, description)
    selected: usize,
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
        let (bg_signal_tx, bg_signal_rx) = watch::channel(());

        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            streaming_text: String::new(),
            streaming_thinking: String::new(),
            is_streaming: false,
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
            committed_msg_idx: 0,
            cmd_palette_idx: 0,
            reasoning_picker: None,
            force_redraw: false,
            esc_pending: false,
            paste_info: None,
            bg_picker: None,
            bg_signal_tx,
            bg_signal_rx,
        }
    }

    /// Build the display text for the input widget.
    /// Shows: text_before + [Pasted Content N chars] + text_after
    fn display_input(&self) -> String {
        if let Some((start, end, ref label)) = self.paste_info {
            let before = &self.input[..start];
            let after = &self.input[end..];
            format!("{before}{label}{after}")
        } else {
            self.input.clone()
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
        let system_prompt = build_system_prompt().await;

        let cwd = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());

        let config = AgentConfig {
            provider: Arc::clone(&self.provider),
            tools: create_default_registry(),
            system_prompt,
            cwd,
            provider_name: self.config.config.provider.to_string(),
            model_name: self.config.effective_model(),
            bg_signal: Some(self.bg_signal_rx.clone()),
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

        // Welcome banner
        self.messages.push(ChatMessage::new(
            "banner",
            build_welcome_banner(self.provider.name(), &self.config.effective_model()),
        ));

        loop {
            // Push overflow messages into the terminal scrollback
            // (skip when session picker is open — insert_before shifts the
            // viewport and leaves visual artifacts over the picker overlay)
            let term_size = terminal.size()?;
            let display_input = self.display_input();
            let cur_input_h = input_height(&display_input, term_size.width, 8);
            let chat_area_h = term_size.height.saturating_sub(cur_input_h + 1);
            if self.session_picker.is_none() && self.bg_picker.is_none() {
                commit_overflow(
                    &self.messages,
                    &mut self.committed_msg_idx,
                    &mut terminal,
                    chat_area_h,
                    term_size.width,
                )?;
            }

            // Pre-compute palette state for this frame
            let filtered_cmds = filtered_commands(&self.input);
            let bg_picker_active = self.bg_picker.is_some();
            let cmd_palette_active = !filtered_cmds.is_empty()
                && !self.is_streaming
                && self.session_picker.is_none()
                && self.model_picker.is_none()
                && self.reasoning_picker.is_none()
                && !bg_picker_active;
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
                let area = frame.area();

                // Dynamic layout: palette below input, above status bar
                let (chat_area, input_area, palette_area, status_area) = if palette_visible {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Min(1),
                            Constraint::Length(input_h),
                            Constraint::Length(palette_h),
                            Constraint::Length(1),
                        ])
                        .split(area);
                    (chunks[0], chunks[1], Some(chunks[2]), chunks[3])
                } else {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Min(1),
                            Constraint::Length(input_h),
                            Constraint::Length(1),
                        ])
                        .split(area);
                    (chunks[0], chunks[1], None, chunks[2])
                };

                // Chat (or overlay pickers)
                if let Some(ref picker) = self.bg_picker {
                    let widget = BgPickerWidget::new(picker);
                    frame.render_widget(widget, chat_area);
                } else if let Some(ref picker) = self.session_picker {
                    let widget = SessionPickerWidget::new(picker);
                    frame.render_widget(widget, chat_area);
                } else {
                    let chat = ChatWidget::new(
                        &self.messages[self.committed_msg_idx..],
                        &self.streaming_text,
                        &self.streaming_thinking,
                        self.is_streaming,
                    );
                    frame.render_widget(chat, chat_area);
                }

                // Palette area: reasoning picker, model picker, or command palette
                if let Some(p_area) = palette_area {
                    if let Some(ref picker) = self.reasoning_picker {
                        let widget = ReasoningPaletteWidget::new(picker);
                        frame.render_widget(widget, p_area);
                    } else if let Some(ref picker) = self.model_picker {
                        let widget = ModelPaletteWidget::new(picker);
                        frame.render_widget(widget, p_area);
                    } else {
                        let widget = CommandPaletteWidget::new(&filtered_cmds, palette_idx);
                        frame.render_widget(widget, p_area);
                    }
                }

                // Input — show paste preview + typed-after text when available
                let di = self.display_input();
                let dc = self.display_cursor();
                let input_widget = if self.bg_picker.is_some() {
                    let hint = match self.bg_picker.as_ref().map(|p| p.view) {
                        Some(BgPickerView::List) => "↑↓ navigate · l=logs · k=kill · ESC close",
                        Some(BgPickerView::Log) => "ESC=back · k=kill",
                        None => "",
                    };
                    InputWidget::new(hint, 0, false)
                } else if self.session_picker.is_some()
                    || self.model_picker.is_some()
                    || self.reasoning_picker.is_some()
                {
                    InputWidget::new("↑↓ navigate · Enter select · ESC close", 0, false)
                } else {
                    InputWidget::new(&di, dc, !self.is_streaming)
                };
                frame.render_widget(input_widget, input_area);

                // ESC hint (rendered inside the input border, right-aligned)
                if self.esc_pending {
                    let hint = " ESC to clear ";
                    let hint_len = hint.len() as u16;
                    if input_area.width > hint_len + 4 {
                        let x = input_area.x + input_area.width - hint_len - 1;
                        let y = input_area.y + input_area.height.saturating_sub(2); // last content row
                        let buf = frame.buffer_mut();
                        buf.set_string(
                            x,
                            y,
                            hint,
                            Style::default().fg(Color::DarkGray),
                        );
                    }
                }

                // Status bar
                let effective_model = self.config.effective_model();
                let status = StatusBarWidget::new(
                    self.provider.name(),
                    &effective_model,
                    self.input_tokens,
                    self.output_tokens,
                    self.is_streaming,
                    self.active_tool.as_deref(),
                );
                frame.render_widget(status, status_area);

                // Set cursor position only when not in picker and not streaming
                if self.bg_picker.is_none()
                    && self.session_picker.is_none()
                    && self.model_picker.is_none()
                    && !self.is_streaming
                {
                    let (cx, cy) = cursor_position(&di, dc, input_area);
                    frame.set_cursor_position((cx, cy));
                }
            })?;

            // Handle events
            tokio::select! {
                Some(event) = event_rx.recv() => {
                    match event {
                        AppEvent::Key(key) => {
                            if self.bg_picker.is_some() {
                                // Background process picker mode
                                match key.code {
                                    KeyCode::Up => {
                                        if let Some(ref mut picker) = self.bg_picker
                                            && picker.view == BgPickerView::List
                                            && picker.selected > 0
                                        {
                                            picker.selected -= 1;
                                        }
                                    }
                                    KeyCode::Down => {
                                        if let Some(ref mut picker) = self.bg_picker
                                            && picker.view == BgPickerView::List
                                            && picker.selected + 1 < picker.entries.len()
                                        {
                                            picker.selected += 1;
                                        }
                                    }
                                    KeyCode::Char('l') | KeyCode::Enter => {
                                        if let Some(ref mut picker) = self.bg_picker
                                            && picker.view == BgPickerView::List
                                        {
                                            picker.load_log();
                                        }
                                    }
                                    KeyCode::Char('k') | KeyCode::Delete => {
                                        if let Some(ref mut picker) = self.bg_picker {
                                            let pid = if picker.view == BgPickerView::Log {
                                                Some(picker.log_pid)
                                            } else {
                                                picker.selected_pid()
                                            };
                                            if let Some(pid) = pid {
                                                // Get command name before killing for the message
                                                let cmd_preview = picker
                                                    .entries
                                                    .iter()
                                                    .find(|e| e.pid == pid)
                                                    .map(|e| {
                                                        if e.command.len() > 40 {
                                                            format!("{}…", &e.command[..39])
                                                        } else {
                                                            e.command.clone()
                                                        }
                                                    })
                                                    .unwrap_or_default();

                                                let was_alive =
                                                    lukan_tools::bg_processes::is_process_alive(pid);
                                                if was_alive {
                                                    lukan_tools::bg_processes::kill_bg_process(pid);
                                                }
                                                // Remove from tracker so it disappears from the list
                                                lukan_tools::bg_processes::remove_bg_process(pid);

                                                // Show confirmation
                                                let action =
                                                    if was_alive { "Killed" } else { "Removed" };
                                                self.messages.push(ChatMessage::new(
                                                    "system",
                                                    format!("{action} process {pid} ({cmd_preview})"),
                                                ));

                                                // Refresh and go back to list view
                                                picker.refresh();
                                                if picker.view == BgPickerView::Log {
                                                    picker.view = BgPickerView::List;
                                                }

                                                // Close picker if no more processes
                                                if picker.entries.is_empty() {
                                                    self.bg_picker = None;
                                                    self.force_redraw = true;
                                                }
                                            }
                                        }
                                    }
                                    KeyCode::Esc => {
                                        if let Some(ref mut picker) = self.bg_picker {
                                            if picker.view == BgPickerView::Log {
                                                picker.view = BgPickerView::List;
                                            } else {
                                                self.bg_picker = None;
                                                self.force_redraw = true;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            } else if self.reasoning_picker.is_some() {
                                // Reasoning effort picker mode
                                match key.code {
                                    KeyCode::Up => {
                                        if let Some(ref mut picker) = self.reasoning_picker
                                            && picker.selected > 0
                                        {
                                            picker.selected -= 1;
                                        }
                                    }
                                    KeyCode::Down => {
                                        if let Some(ref mut picker) = self.reasoning_picker
                                            && picker.selected + 1 < picker.levels.len()
                                        {
                                            picker.selected += 1;
                                        }
                                    }
                                    KeyCode::Enter => {
                                        let picker = self.reasoning_picker.take().unwrap();
                                        let (effort, _, _) = picker.levels[picker.selected];
                                        self.apply_model_switch_with_effort(
                                            &picker.model_entry,
                                            Some(effort),
                                        )
                                        .await;
                                        self.force_redraw = true;
                                    }
                                    KeyCode::Esc => {
                                        self.reasoning_picker = None;
                                        self.force_redraw = true;
                                    }
                                    _ => {}
                                }
                            } else if self.session_picker.is_some() {
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
                                        self.force_redraw = true;
                                    }
                                    KeyCode::Esc => {
                                        self.session_picker = None;
                                        self.force_redraw = true;
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
                                        self.force_redraw = true;
                                    }
                                    KeyCode::Char('d') => {
                                        // Set selected model as default and switch to it
                                        let idx = self.model_picker.as_ref().unwrap().selected;
                                        self.set_default_model(idx).await;
                                        self.model_picker = None;
                                        self.force_redraw = true;
                                    }
                                    KeyCode::Esc => {
                                        self.model_picker = None;
                                        self.force_redraw = true;
                                    }
                                    _ => {}
                                }
                            } else if is_quit(&key) {
                                self.should_quit = true;
                            } else if key.code == KeyCode::Char('b')
                                && key.modifiers.contains(crossterm::event::KeyModifiers::ALT)
                                && self.is_streaming
                            {
                                // Alt+B: send running Bash command to background
                                let _ = self.bg_signal_tx.send(());
                                self.messages.push(ChatMessage::new(
                                    "system",
                                    "Sending current command to background...",
                                ));
                            } else if !self.is_streaming {
                                let cmds = filtered_commands(&self.input);
                                let has_palette = !cmds.is_empty()
                                    && self.session_picker.is_none()
                                    && self.model_picker.is_none();

                                match key.code {
                                    KeyCode::Up if has_palette => {
                                        if self.cmd_palette_idx > 0 {
                                            self.cmd_palette_idx -= 1;
                                        } else {
                                            self.cmd_palette_idx =
                                                cmds.len().saturating_sub(1);
                                        }
                                    }
                                    KeyCode::Down if has_palette => {
                                        self.cmd_palette_idx =
                                            (self.cmd_palette_idx + 1) % cmds.len().max(1);
                                    }
                                    KeyCode::Esc => {
                                        if has_palette && !self.esc_pending {
                                            self.input.clear();
                                            self.cursor_pos = 0;
                                            self.cmd_palette_idx = 0;
                                            self.paste_info = None;
                                        } else if self.esc_pending {
                                            self.input.clear();
                                            self.cursor_pos = 0;
                                            self.cmd_palette_idx = 0;
                                            self.esc_pending = false;
                                            self.paste_info = None;
                                        } else if !self.input.is_empty() {
                                            self.esc_pending = true;
                                        }
                                    }
                                    KeyCode::Enter => {
                                        if has_palette {
                                            let idx = self.cmd_palette_idx
                                                .min(cmds.len().saturating_sub(1));
                                            self.input = cmds[idx].0.to_string();
                                            self.cursor_pos = self.input.len();
                                            self.cmd_palette_idx = 0;
                                        }
                                        if !self.input.trim().is_empty() {
                                            self.submit_message(agent_tx.clone()).await;
                                        }
                                    }
                                    KeyCode::Char(c) => {
                                        let clen = c.len_utf8();
                                        self.input.insert(self.cursor_pos, c);
                                        // Shift paste boundaries if inserting before/at start
                                        if let Some((ref mut s, ref mut e, _)) = self.paste_info
                                            && self.cursor_pos <= *s
                                        {
                                            *s += clen;
                                            *e += clen;
                                        }
                                        self.cursor_pos += clen;
                                        self.cmd_palette_idx = 0;
                                        self.esc_pending = false;
                                    }
                                    KeyCode::Backspace => {
                                        if let Some((ps, pe, _)) = self.paste_info {
                                            if self.cursor_pos == pe {
                                                // At paste end → delete entire paste block
                                                self.input.drain(ps..pe);
                                                self.cursor_pos = ps;
                                                self.paste_info = None;
                                            } else if self.cursor_pos > pe && self.cursor_pos > 0 {
                                                // After paste block — normal delete
                                                let prev = self.input[..self.cursor_pos]
                                                    .char_indices().next_back()
                                                    .map(|(i, _)| i).unwrap_or(0);
                                                self.input.drain(prev..self.cursor_pos);
                                                self.cursor_pos = prev;
                                            } else if self.cursor_pos <= ps && self.cursor_pos > 0 {
                                                // Before paste block — normal delete, shift paste
                                                let prev = self.input[..self.cursor_pos]
                                                    .char_indices().next_back()
                                                    .map(|(i, _)| i).unwrap_or(0);
                                                let removed = self.cursor_pos - prev;
                                                self.input.drain(prev..self.cursor_pos);
                                                self.cursor_pos = prev;
                                                if let Some((ref mut s, ref mut e, _)) = self.paste_info {
                                                    *s -= removed;
                                                    *e -= removed;
                                                }
                                            }
                                        } else if self.cursor_pos > 0 {
                                            let prev = self.input[..self.cursor_pos]
                                                .char_indices().next_back()
                                                .map(|(i, _)| i).unwrap_or(0);
                                            self.input.drain(prev..self.cursor_pos);
                                            self.cursor_pos = prev;
                                        }
                                        self.cmd_palette_idx = 0;
                                        self.esc_pending = false;
                                    }
                                    KeyCode::Left => {
                                        if self.cursor_pos > 0 {
                                            if let Some((ps, pe, _)) = self.paste_info
                                                && self.cursor_pos > ps && self.cursor_pos <= pe
                                            {
                                                // Jump over paste block
                                                self.cursor_pos = ps;
                                            } else {
                                                self.cursor_pos = self.input[..self.cursor_pos]
                                                    .char_indices().next_back()
                                                    .map(|(i, _)| i).unwrap_or(0);
                                            }
                                        }
                                    }
                                    KeyCode::Right => {
                                        if self.cursor_pos < self.input.len() {
                                            if let Some((ps, pe, _)) = self.paste_info
                                                && self.cursor_pos >= ps && self.cursor_pos < pe
                                            {
                                                // Jump over paste block
                                                self.cursor_pos = pe;
                                            } else {
                                                self.cursor_pos = self.input[self.cursor_pos..]
                                                    .char_indices().nth(1)
                                                    .map(|(i, _)| self.cursor_pos + i)
                                                    .unwrap_or(self.input.len());
                                            }
                                        }
                                    }
                                    KeyCode::Home => self.cursor_pos = 0,
                                    KeyCode::End => self.cursor_pos = self.input.len(),
                                    _ => {}
                                }
                            }
                        }
                        AppEvent::Paste(text) => {
                            if !self.is_streaming
                                && self.session_picker.is_none()
                                && self.model_picker.is_none()
                                && self.reasoning_picker.is_none()
                            {
                                let char_count = text.len();
                                let label = format!("[Pasted Content {char_count} chars]");
                                let start = self.cursor_pos;
                                self.input.insert_str(start, &text);
                                let end = start + text.len();
                                self.cursor_pos = end;
                                self.paste_info = Some((start, end, label));
                                self.esc_pending = false;
                                self.cmd_palette_idx = 0;
                            }
                        }
                        AppEvent::Resize(_, _) => {}
                        AppEvent::Tick => {
                            // Auto-refresh bg_picker log view
                            if let Some(ref mut picker) = self.bg_picker {
                                picker.refresh_log();
                            }
                        }
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

        // Clean up background processes before exiting
        lukan_tools::bg_processes::cleanup_all();

        stdout().execute(crossterm::event::DisableBracketedPaste)?;
        disable_raw_mode()?;
        println!(); // new line so the shell prompt starts clean
        Ok(())
    }

    async fn submit_message(&mut self, agent_tx: mpsc::Sender<StreamEvent>) {
        let text = self.input.trim().to_string();
        let display = self.display_input().trim().to_string();
        self.input.clear();
        self.cursor_pos = 0;
        self.paste_info = None;

        // Handle /exit
        if text == "/exit" {
            self.should_quit = true;
            return;
        }

        // Handle /resume command
        if text == "/resume" {
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
            self.committed_msg_idx = 0;
            self.input_tokens = 0;
            self.output_tokens = 0;
            // Reset agent — a new session will be created on next message
            self.agent = None;
            self.session_id = None;
            return;
        }

        // Handle /compact
        if text == "/compact" {
            if self.is_streaming {
                self.messages.push(ChatMessage::new(
                    "system",
                    "Cannot compact while a response is streaming.",
                ));
                return;
            }
            let agent = match self.agent.take() {
                Some(a) => a,
                None => {
                    self.messages
                        .push(ChatMessage::new("system", "No active session to compact."));
                    return;
                }
            };
            self.is_streaming = true;
            let msg_before = agent.message_count();
            let (return_tx, return_rx) = tokio::sync::oneshot::channel::<AgentLoop>();
            self.agent_return_rx = Some(return_rx);
            let tx = agent_tx.clone();
            let mut agent = agent;
            tokio::spawn(async move {
                if let Err(e) = agent.compact(tx.clone()).await {
                    let _ = tx
                        .send(StreamEvent::Error {
                            error: e.to_string(),
                        })
                        .await;
                } else {
                    let msg_after = agent.message_count();
                    let summary = format!(
                        "Compacted: {} messages → {} messages.",
                        msg_before, msg_after
                    );
                    // TextDelta must come before MessageEnd so it gets flushed
                    // into messages when MessageEnd sets is_streaming = false.
                    let _ = tx.send(StreamEvent::TextDelta { text: summary }).await;
                    let _ = tx
                        .send(StreamEvent::MessageEnd {
                            stop_reason: lukan_core::models::events::StopReason::EndTurn,
                        })
                        .await;
                }
                let _ = return_tx.send(agent);
            });
            return;
        }

        // Handle /memories [activate | deactivate | add <text>]
        if text == "/memories" || text.starts_with("/memories ") {
            let sub = text.strip_prefix("/memories").unwrap_or("").trim().to_string();
            let memory_path = LukanPaths::global_memory_file();
            let mut did_change = false;
            if sub == "activate" {
                if !memory_path.exists() {
                    if let Some(parent) = memory_path.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    let _ = tokio::fs::write(&memory_path, "# Project Memory\n\n").await;
                }
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Memory activated: {}", memory_path.display()),
                ));
                did_change = true;
            } else if sub == "deactivate" {
                let _ = tokio::fs::remove_file(&memory_path).await;
                self.messages
                    .push(ChatMessage::new("system", "Memory deactivated. File removed."));
                did_change = true;
            } else if sub.starts_with("add") {
                let entry = sub.strip_prefix("add").unwrap_or("").trim().to_string();
                if entry.is_empty() {
                    self.messages.push(ChatMessage::new(
                        "system",
                        "Usage: /memories add <text>",
                    ));
                } else {
                    let current = tokio::fs::read_to_string(&memory_path)
                        .await
                        .unwrap_or_else(|_| "# Project Memory\n\n".to_string());
                    let updated = format!("{current}\n- {entry}\n");
                    let _ = tokio::fs::write(&memory_path, &updated).await;
                    self.messages.push(ChatMessage::new(
                        "system",
                        format!("Memory updated: \"{entry}\""),
                    ));
                    did_change = true;
                }
            } else {
                let active = memory_path.exists();
                self.messages.push(ChatMessage::new(
                    "system",
                    format!(
                        "Memory: {}. Usage: /memories activate | deactivate | add <text>",
                        if active { "active" } else { "inactive" }
                    ),
                ));
            }
            if did_change
                && let Some(agent) = self.agent.as_mut()
            {
                agent.reload_system_prompt(build_system_prompt().await);
            }
            return;
        }

        // Handle /gmemory [show | add <text> | clear]
        if text == "/gmemory" || text.starts_with("/gmemory ") {
            let sub = text.strip_prefix("/gmemory").unwrap_or("").trim().to_string();
            let memory_path = LukanPaths::global_memory_file();
            let mut did_change = false;
            if sub == "show" {
                let content = tokio::fs::read_to_string(&memory_path)
                    .await
                    .unwrap_or_else(|_| "(empty)".to_string());
                self.messages
                    .push(ChatMessage::new("system", format!("Memory:\n{content}")));
            } else if sub.starts_with("add ") {
                let entry = sub.strip_prefix("add ").unwrap_or("").trim().to_string();
                if entry.is_empty() {
                    self.messages
                        .push(ChatMessage::new("system", "Usage: /gmemory add <text>"));
                } else {
                    let current = tokio::fs::read_to_string(&memory_path)
                        .await
                        .unwrap_or_else(|_| "# Project Memory\n\n".to_string());
                    let updated = format!("{current}\n- {entry}\n");
                    let _ = tokio::fs::write(&memory_path, &updated).await;
                    self.messages.push(ChatMessage::new(
                        "system",
                        format!("Memory updated: \"{entry}\""),
                    ));
                    did_change = true;
                }
            } else if sub == "clear" {
                let _ = tokio::fs::write(&memory_path, "# Project Memory\n\n").await;
                self.messages
                    .push(ChatMessage::new("system", "Memory cleared."));
                did_change = true;
            } else {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!(
                        "Global memory: {}\nUsage: /gmemory show | add <text> | clear",
                        memory_path.display()
                    ),
                ));
            }
            if did_change
                && let Some(agent) = self.agent.as_mut()
            {
                agent.reload_system_prompt(build_system_prompt().await);
            }
            return;
        }

        // Handle /bg
        if text == "/bg" {
            let processes = lukan_tools::bg_processes::get_bg_processes();
            if processes.is_empty() {
                self.messages.push(ChatMessage::new(
                    "system",
                    "No background processes.",
                ));
            } else {
                let entries: Vec<BgEntry> = processes
                    .into_iter()
                    .map(|(pid, command, started_at, alive)| BgEntry {
                        pid,
                        command,
                        started_at,
                        alive,
                    })
                    .collect();
                self.bg_picker = Some(BgPicker::new(entries));
            }
            return;
        }

        // Handle /checkpoints
        if text == "/checkpoints" {
            let checkpoints = self
                .agent
                .as_ref()
                .map(|a| a.checkpoints().to_vec())
                .unwrap_or_default();
            if checkpoints.is_empty() {
                self.messages.push(ChatMessage::new(
                    "system",
                    "No checkpoints in this session.",
                ));
            } else {
                let list = checkpoints
                    .iter()
                    .map(|c| {
                        let files = c.snapshots.len();
                        format!("  {} — {} ({files} file{})", c.id, c.message, if files == 1 { "" } else { "s" })
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Checkpoints ({}):\n{list}", checkpoints.len()),
                ));
            }
            return;
        }

        // Regular message — show truncated preview in chat, send full text to agent
        self.messages.push(ChatMessage::new("user", display));

        self.is_streaming = true;
        self.streaming_text.clear();
        self.streaming_thinking.clear();
        self.active_tool = None;

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

        let system_prompt = build_system_prompt().await;
        let cwd = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());

        let config = AgentConfig {
            provider: Arc::clone(&self.provider),
            tools: create_default_registry(),
            system_prompt,
            cwd,
            provider_name: self.config.config.provider.to_string(),
            model_name: self.config.effective_model(),
            bg_signal: Some(self.bg_signal_rx.clone()),
        };

        match AgentLoop::load_session(config, &session_id).await {
            Ok(agent) => {
                // Rebuild UI messages from the loaded session
                self.messages.clear();
                self.committed_msg_idx = 0;

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

    /// Switch to the selected model from the picker.
    /// For codex models, opens the reasoning effort picker first.
    async fn select_model(&mut self, idx: usize) {
        let picker = self.model_picker.as_ref().unwrap();
        let entry = picker.models[idx].clone();

        let Some((provider_str, _model_name)) = entry.split_once(':') else {
            self.messages.push(ChatMessage::new(
                "system",
                format!("Invalid model format: {entry}"),
            ));
            return;
        };

        // For codex models, show reasoning effort picker first
        if provider_str == "openai-codex" {
            let current_effort = self.provider.reasoning_effort().unwrap_or("medium");
            let default_idx = match current_effort {
                "low" => 0,
                "high" => 2,
                "extra_high" => 3,
                _ => 1, // medium
            };
            self.reasoning_picker = Some(ReasoningPicker {
                model_entry: entry,
                levels: vec![
                    ("low", "Low", "Fast responses with lighter reasoning"),
                    (
                        "medium",
                        "Medium (default)",
                        "Balances speed and reasoning depth",
                    ),
                    (
                        "high",
                        "High",
                        "Greater reasoning depth for complex problems",
                    ),
                    ("extra_high", "Extra high", "Maximum reasoning depth"),
                ],
                selected: default_idx,
            });
            self.model_picker = None;
            return;
        }

        // Non-codex: switch immediately
        self.apply_model_switch(&entry).await;
    }

    /// Set the selected model as the default (persisted to config.json) and switch to it.
    async fn set_default_model(&mut self, idx: usize) {
        let picker = self.model_picker.as_ref().unwrap();
        let entry = picker.models[idx].clone();

        let Some((provider_str, model_name)) = entry.split_once(':') else {
            self.messages.push(ChatMessage::new(
                "system",
                format!("Invalid model format: {entry}"),
            ));
            return;
        };

        // Update config and persist
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

        self.config.config.provider = provider_name;
        self.config.config.model = Some(model_name.to_string());

        if let Err(e) = ConfigManager::save(&self.config.config).await {
            self.messages.push(ChatMessage::new(
                "system",
                format!("Failed to save config: {e}"),
            ));
            return;
        }

        // Switch to the model
        self.apply_model_switch(&entry).await;
        self.messages.push(ChatMessage::new(
            "system",
            format!("Default model set to {entry}"),
        ));
    }

    /// Apply the model switch after all selections are done.
    async fn apply_model_switch(&mut self, entry: &str) {
        self.apply_model_switch_with_effort(entry, None).await;
    }

    /// Apply the model switch, optionally setting reasoning effort.
    async fn apply_model_switch_with_effort(
        &mut self,
        entry: &str,
        reasoning_effort: Option<&str>,
    ) {
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
                if let Some(effort) = reasoning_effort {
                    new_provider.set_reasoning_effort(effort);
                }
                self.provider = Arc::from(new_provider);
                self.config = new_config;
                // Swap provider in existing agent to preserve history,
                // or reset if no agent exists yet.
                if let Some(agent) = self.agent.as_mut() {
                    agent.swap_provider(Arc::clone(&self.provider));
                }
                if let Some(banner) = self.messages.iter_mut().find(|m| m.role == "banner") {
                    let new_banner =
                        build_welcome_banner(self.provider.name(), &self.config.effective_model());
                    banner.content = sanitize_for_display(&new_banner);
                }
                let effort_label = reasoning_effort
                    .map(|e| format!(" (reasoning: {e})"))
                    .unwrap_or_default();
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Switched to {entry}{effort_label}"),
                ));
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
                self.streaming_thinking.clear();
                self.active_tool = None;
            }
            StreamEvent::TextDelta { text } => {
                self.streaming_text.push_str(&text);
            }
            StreamEvent::ThinkingDelta { text } => {
                self.streaming_thinking.push_str(&text);
            }
            StreamEvent::ToolUseStart { name, .. } => {
                // Flush current text as a message before tool call
                self.active_tool = Some(name.clone());
                if !self.streaming_text.is_empty() {
                    let content = std::mem::take(&mut self.streaming_text);
                    self.messages.push(ChatMessage::new("assistant", content));
                }
            }
            StreamEvent::ToolUseEnd { id, name, input } => {
                // ● ToolName(input summary)
                let summary = summarize_tool_input(&name, &input);
                let mut msg = ChatMessage::new("tool_call", format!("● {name}({summary})"));
                msg.tool_id = Some(id);
                self.messages.push(msg);
            }
            StreamEvent::ToolProgress { id, name, content } => {
                self.active_tool = Some(name);
                let sanitized = sanitize_for_display(&content);
                let insert_pos = self.tool_insert_position(&id);

                // Try to consolidate with existing progress for this tool
                if insert_pos > 0 {
                    let prev = &self.messages[insert_pos - 1];
                    if prev.role == "tool_result"
                        && prev.tool_id.as_deref() == Some(&*id)
                        && prev.diff.is_none()
                    {
                        let prev = &mut self.messages[insert_pos - 1];
                        prev.content.push('\n');
                        prev.content.push_str(&format!("     {sanitized}"));
                        return;
                    }
                }

                let mut msg = ChatMessage::new("tool_result", format!("  ⎿  {content}"));
                msg.tool_id = Some(id);
                self.messages.insert(insert_pos, msg);
            }
            StreamEvent::ToolResult {
                id,
                content,
                is_error,
                diff,
                ..
            } => {
                self.active_tool = None;
                let formatted = format_tool_result(&content, is_error.unwrap_or(false));
                let insert_pos = self.tool_insert_position(&id);
                let mut msg = ChatMessage::with_diff("tool_result", formatted, diff);
                msg.tool_id = Some(id);
                self.messages.insert(insert_pos, msg);
            }
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
                ..
            } => {
                self.input_tokens += input_tokens;
                self.output_tokens += output_tokens;
            }
            StreamEvent::MessageEnd { stop_reason } => {
                if !self.streaming_text.is_empty() {
                    let content = std::mem::take(&mut self.streaming_text);
                    self.messages.push(ChatMessage::new("assistant", content));
                }
                // When stop_reason is ToolUse, tools are about to execute —
                // keep is_streaming=true so Alt+B works and the UI shows
                // "streaming" status. ToolResult events will follow, and
                // the final MessageEnd (with EndTurn) will set it to false.
                if stop_reason != StopReason::ToolUse {
                    self.is_streaming = false;
                    self.active_tool = None;
                }
            }
            StreamEvent::Error { error } => {
                self.messages
                    .push(ChatMessage::new("assistant", format!("Error: {error}")));
                self.is_streaming = false;
            }
            _ => {}
        }
    }

    /// Find the insertion position for a tool result: right after the
    /// tool_call with this ID and any existing results for it.
    fn tool_insert_position(&self, tool_id: &str) -> usize {
        // Find the tool_call with this ID
        let call_idx = self.messages.iter().rposition(|m| {
            m.role == "tool_call" && m.tool_id.as_deref() == Some(tool_id)
        });
        match call_idx {
            Some(idx) => {
                // Scan forward past any messages already belonging to this tool
                let mut pos = idx + 1;
                while pos < self.messages.len()
                    && self.messages[pos].tool_id.as_deref() == Some(tool_id)
                {
                    pos += 1;
                }
                pos
            }
            None => self.messages.len(), // fallback: append
        }
    }
}

// ── Inline Viewport: commit overflow to scrollback ─────────────────────

/// Push completed messages that no longer fit in the chat area into the
/// terminal's native scrollback via `insert_before`. Only whole messages
/// are committed — streaming text is never pushed.
fn commit_overflow(
    messages: &[ChatMessage],
    committed_msg_idx: &mut usize,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    chat_area_h: u16,
    width: u16,
) -> Result<()> {
    if *committed_msg_idx >= messages.len() {
        return Ok(());
    }

    let uncommitted = &messages[*committed_msg_idx..];
    let all_lines = build_message_lines(uncommitted, "", "", false);
    let total_rows = physical_row_count(&all_lines, width);

    if total_rows <= chat_area_h {
        return Ok(());
    }

    // Find how many messages to commit so the remainder fits
    let rows_to_free = total_rows - chat_area_h;
    let mut rows_acc: u16 = 0;
    let mut msgs_to_commit = 0;

    for msg in uncommitted {
        if rows_acc >= rows_to_free {
            break;
        }
        let msg_lines = build_message_lines(std::slice::from_ref(msg), "", "", false);
        rows_acc += physical_row_count(&msg_lines, width);
        msgs_to_commit += 1;
    }

    if msgs_to_commit > 0 && rows_acc > 0 {
        let commit_msgs = &messages[*committed_msg_idx..*committed_msg_idx + msgs_to_commit];
        let commit_lines = build_message_lines(commit_msgs, "", "", false);
        let commit_rows = physical_row_count(&commit_lines, width);

        use ratatui::widgets::{Paragraph, Widget, Wrap};
        terminal.insert_before(commit_rows, |buf| {
            let p = Paragraph::new(commit_lines).wrap(Wrap { trim: false });
            p.render(buf.area, buf);
        })?;

        *committed_msg_idx += msgs_to_commit;
    }
    Ok(())
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
        "
 ██╗     ██╗   ██╗██╗  ██╗ █████╗ ███╗   ██╗
 ██║     ██║   ██║██║ ██╔╝██╔══██╗████╗  ██║   AI Agent CLI
 ██║     ██║   ██║█████╔╝ ███████║██╔██╗ ██║   {provider} > {model}
 ██║     ██║   ██║██╔═██╗ ██╔══██║██║╚██╗██║
 ███████╗╚██████╔╝██║  ██╗██║  ██║██║ ╚████║   {cwd}
 ╚══════╝ ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═══╝

 /model  Switch model    /resume  Sessions    /bg  Background    /clear  Clear    Alt+B  Background cmd    Ctrl+C  Quit"
    )
}

// ── Command Palette ──────────────────────────────────────────────────────

const COMMANDS: &[(&str, &str)] = &[
    ("/model", "choose model to use"),
    ("/resume", "resume a saved session"),
    ("/bg", "view and manage background processes"),
    ("/clear", "clear chat and start fresh"),
    ("/compact", "compact conversation history"),
    ("/memories", "manage project memory (activate | deactivate | add <text>)"),
    ("/gmemory", "global memory (show | add <text> | clear)"),
    ("/checkpoints", "list session checkpoints"),
    ("/exit", "quit lukan"),
];

// ── System Prompt Builder ─────────────────────────────────────────────────

/// Build the system prompt, appending MEMORY.md content if it exists.
async fn build_system_prompt() -> SystemPrompt {
    const BASE: &str = include_str!("../../../prompts/base.txt");
    let memory_path = LukanPaths::global_memory_file();
    if let Ok(memory) = tokio::fs::read_to_string(&memory_path).await {
        let trimmed = memory.trim();
        if !trimmed.is_empty() {
            let combined = format!("{BASE}\n\n## Project Memory\n\n{trimmed}");
            return SystemPrompt::Text(combined);
        }
    }
    SystemPrompt::Text(BASE.to_string())
}

/// Filter available commands by the current input prefix.
/// Returns empty if input doesn't start with `/` or contains a space.
fn filtered_commands(input: &str) -> Vec<(&'static str, &'static str)> {
    if !input.starts_with('/') || input.contains(' ') {
        return vec![];
    }
    COMMANDS
        .iter()
        .filter(|(cmd, _)| cmd.starts_with(input))
        .copied()
        .collect()
}

struct CommandPaletteWidget<'a> {
    commands: &'a [(&'static str, &'static str)],
    selected: usize,
}

impl<'a> CommandPaletteWidget<'a> {
    fn new(commands: &'a [(&'static str, &'static str)], selected: usize) -> Self {
        Self { commands, selected }
    }
}

impl Widget for CommandPaletteWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let mut lines: Vec<Line<'_>> = Vec::new();

        // Blank separator line at top
        lines.push(Line::from(""));

        for (i, (cmd, desc)) in self.commands.iter().enumerate() {
            let is_selected = i == self.selected;
            let pointer = if is_selected { "▸ " } else { "  " };

            // Pad command name to align descriptions
            let padded_cmd = format!("{cmd:<14}");

            if is_selected {
                lines.push(Line::from(vec![
                    Span::styled(
                        pointer,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        padded_cmd,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled((*desc).to_string(), Style::default().fg(Color::White)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(Color::DarkGray)),
                    Span::styled(padded_cmd, Style::default().fg(Color::Gray)),
                    Span::styled((*desc).to_string(), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

// ── Reasoning Palette Widget ─────────────────────────────────────────────

struct ReasoningPaletteWidget<'a> {
    picker: &'a ReasoningPicker,
}

impl<'a> ReasoningPaletteWidget<'a> {
    fn new(picker: &'a ReasoningPicker) -> Self {
        Self { picker }
    }
}

impl Widget for ReasoningPaletteWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let mut lines: Vec<Line<'_>> = Vec::new();

        // Header with model name
        let model_name = self
            .picker
            .model_entry
            .split_once(':')
            .map(|(_, m)| m)
            .unwrap_or(&self.picker.model_entry);
        lines.push(Line::from(Span::styled(
            format!("  Select Reasoning Level for {model_name}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        for (i, (_value, label, desc)) in self.picker.levels.iter().enumerate() {
            let is_selected = i == self.picker.selected;
            let pointer = if is_selected { "▸ " } else { "  " };
            let num = format!("{}. ", i + 1);
            let padded_label = format!("{label:<20}");

            if is_selected {
                lines.push(Line::from(vec![
                    Span::styled(
                        pointer,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        num,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        padded_label,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled((*desc).to_string(), Style::default().fg(Color::Gray)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(Color::DarkGray)),
                    Span::styled(num, Style::default().fg(Color::DarkGray)),
                    Span::styled(padded_label, Style::default().fg(Color::Gray)),
                    Span::styled((*desc).to_string(), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

// ── Model Palette Widget ─────────────────────────────────────────────────

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget},
};

struct ModelPaletteWidget<'a> {
    picker: &'a ModelPicker,
}

impl<'a> ModelPaletteWidget<'a> {
    fn new(picker: &'a ModelPicker) -> Self {
        Self { picker }
    }
}

impl Widget for ModelPaletteWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let mut lines: Vec<Line<'_>> = Vec::new();

        // Header
        lines.push(Line::from(vec![
            Span::styled(
                "  Select Model",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  (d = set as default)",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        lines.push(Line::from(""));

        for (i, entry) in self.picker.models.iter().enumerate() {
            let is_selected = i == self.picker.selected;
            let is_current = *entry == self.picker.current;

            let pointer = if is_selected { "▸ " } else { "  " };
            let num = format!("{}. ", i + 1);

            let suffix = if is_current { " (current)" } else { "" };

            if is_selected {
                lines.push(Line::from(vec![
                    Span::styled(
                        pointer,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        num,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        entry.clone(),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(suffix, Style::default().fg(Color::Green)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(Color::DarkGray)),
                    Span::styled(num, Style::default().fg(Color::DarkGray)),
                    Span::styled(entry.clone(), Style::default().fg(Color::Gray)),
                    Span::styled(suffix, Style::default().fg(Color::Green)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines);
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
        Clear.render(area, buf);
        let mut lines: Vec<Line<'_>> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Resume Session",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // Each session takes 1 line
        let available_rows = area.height.saturating_sub(3) as usize; // minus header + blank + scroll indicator
        let visible_items = available_rows.max(1);
        let total = self.picker.sessions.len();
        let selected = self.picker.selected;

        // Scroll offset: keep selected item visible
        let scroll_offset = if selected >= visible_items {
            selected - visible_items + 1
        } else {
            0
        };
        let end = (scroll_offset + visible_items).min(total);

        for i in scroll_offset..end {
            let session = &self.picker.sessions[i];
            let is_selected = i == selected;
            let is_current = self
                .picker
                .current_id
                .as_ref()
                .is_some_and(|id| *id == session.id);

            let pointer = if is_selected { "▸ " } else { "  " };

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

            // Last message preview on the right, dimmed
            if let Some(ref preview) = session.last_message {
                spans.push(Span::styled(
                    format!(" · {preview}"),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            lines.push(Line::from(spans));
        }

        // Scroll indicator
        if total > visible_items {
            lines.push(Line::from(Span::styled(
                format!("  ({}/{total})", selected + 1),
                Style::default().fg(Color::DarkGray),
            )));
        }

        let paragraph = Paragraph::new(lines);
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
