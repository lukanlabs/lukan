use anyhow::Result;
use crossterm::{
    ExecutableCommand,
    event::KeyCode,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
};
use std::io::stdout;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::error;

use lukan_core::config::{ConfigManager, CredentialsManager, ProviderName, ResolvedConfig};
use lukan_core::models::events::StreamEvent;
use lukan_core::models::messages::Message;
use lukan_providers::{Provider, StreamParams, SystemPrompt, create_provider};

use crate::event::{AppEvent, is_quit, spawn_event_reader};
use crate::widgets::chat::{ChatMessage, ChatWidget};
use crate::widgets::input::InputWidget;
use crate::widgets::status_bar::StatusBarWidget;

/// Application state
pub struct App {
    messages: Vec<ChatMessage>,
    history: Vec<Message>,
    input: String,
    cursor_pos: usize,
    streaming_text: String,
    is_streaming: bool,
    scroll_offset: u16,
    input_tokens: u64,
    output_tokens: u64,
    provider: Arc<dyn Provider>,
    config: ResolvedConfig,
    should_quit: bool,
    /// Model picker state
    model_picker: Option<ModelPicker>,
}

/// Interactive model picker state
struct ModelPicker {
    models: Vec<String>,
    selected: usize,
    current: String,
}

impl App {
    pub fn new(provider: Box<dyn Provider>, config: ResolvedConfig) -> Self {
        Self {
            messages: Vec::new(),
            history: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            streaming_text: String::new(),
            is_streaming: false,
            scroll_offset: 0,
            input_tokens: 0,
            output_tokens: 0,
            provider: Arc::from(provider),
            config,
            should_quit: false,
            model_picker: None,
        }
    }

    pub async fn run(mut self) -> Result<()> {
        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AppEvent>();
        spawn_event_reader(event_tx);

        let (agent_tx, mut agent_rx) = mpsc::channel::<StreamEvent>(256);

        // Welcome banner
        self.messages.push(ChatMessage {
            role: "banner".to_string(),
            content: build_welcome_banner(self.provider.name(), &self.config.effective_model()),
        });

        loop {
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

                // Chat (or model picker overlay)
                if let Some(ref picker) = self.model_picker {
                    let widget = ModelPickerWidget::new(picker);
                    frame.render_widget(widget, chunks[0]);
                } else {
                    let chat =
                        ChatWidget::new(&self.messages, &self.streaming_text, self.scroll_offset);
                    frame.render_widget(chat, chunks[0]);
                }

                // Input
                let input_widget = if self.model_picker.is_some() {
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
                if self.model_picker.is_none() && !self.is_streaming {
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
                            if self.model_picker.is_some() {
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
                        AppEvent::Resize(_, _) => {}
                        AppEvent::Tick => {}
                    }
                }
                Some(stream_event) = agent_rx.recv() => {
                    self.handle_stream_event(stream_event);
                }
            }

            if self.should_quit {
                break;
            }
        }

        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;
        Ok(())
    }

    async fn submit_message(&mut self, agent_tx: mpsc::Sender<StreamEvent>) {
        let text = self.input.trim().to_string();
        self.input.clear();
        self.cursor_pos = 0;

        // Handle /model command
        if text == "/model" || text.starts_with("/model ") {
            self.open_model_picker().await;
            return;
        }

        // Handle /clear
        if text == "/clear" {
            self.messages.clear();
            self.history.clear();
            self.input_tokens = 0;
            self.output_tokens = 0;
            return;
        }

        // Regular message
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: text.clone(),
        });
        self.history.push(Message::user(&text));

        self.is_streaming = true;
        self.streaming_text.clear();

        let system_prompt =
            SystemPrompt::Text(include_str!("../../../prompts/base.txt").to_string());

        let params = StreamParams {
            system_prompt,
            messages: self.history.clone(),
            tools: Vec::new(),
        };

        let provider = Arc::clone(&self.provider);
        tokio::spawn(async move {
            if let Err(e) = provider.stream(params, agent_tx.clone()).await {
                error!("Provider stream error: {e}");
                agent_tx
                    .send(StreamEvent::Error {
                        error: e.to_string(),
                    })
                    .await
                    .ok();
            }
        });
    }

    /// Open the interactive model picker
    async fn open_model_picker(&mut self) {
        let models = match ConfigManager::get_models().await {
            Ok(m) => m,
            Err(e) => {
                self.messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: format!("Failed to load models: {e}"),
                });
                return;
            }
        };

        if models.is_empty() {
            self.messages.push(ChatMessage {
                role: "system".to_string(),
                content: "No models available. Run 'lukan setup' to configure providers."
                    .to_string(),
            });
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
            self.messages.push(ChatMessage {
                role: "system".to_string(),
                content: format!("Invalid model format: {entry}"),
            });
            return;
        };

        let provider_name: ProviderName =
            match serde_json::from_value(serde_json::Value::String(provider_str.to_string())) {
                Ok(p) => p,
                Err(_) => {
                    self.messages.push(ChatMessage {
                        role: "system".to_string(),
                        content: format!("Unknown provider: {provider_str}"),
                    });
                    return;
                }
            };

        let credentials = match CredentialsManager::load().await {
            Ok(c) => c,
            Err(e) => {
                self.messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: format!("Failed to load credentials: {e}"),
                });
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
                self.messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: format!("Switched to {entry}"),
                });
            }
            Err(e) => {
                self.messages.push(ChatMessage {
                    role: "system".to_string(),
                    content: format!("Failed to switch: {e}"),
                });
            }
        }
    }

    fn handle_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::MessageStart => {
                self.streaming_text.clear();
            }
            StreamEvent::TextDelta { text } => {
                self.streaming_text.push_str(&text);
            }
            StreamEvent::ThinkingDelta { text } => {
                self.streaming_text.push_str(&text);
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
                    self.messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        content: content.clone(),
                    });
                    self.history.push(Message::assistant(&content));
                }
                self.is_streaming = false;
            }
            StreamEvent::Error { error } => {
                self.messages.push(ChatMessage {
                    role: "assistant".to_string(),
                    content: format!("Error: {error}"),
                });
                self.is_streaming = false;
            }
            _ => {}
        }
    }
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

 /model  Switch AI model    /clear  Clear chat    Ctrl+C  Quit"
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
