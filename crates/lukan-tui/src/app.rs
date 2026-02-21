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

use lukan_core::config::ResolvedConfig;
use lukan_core::models::events::StreamEvent;
use lukan_core::models::messages::Message;
use lukan_providers::{Provider, StreamParams, SystemPrompt};

use crate::event::{AppEvent, is_quit, spawn_event_reader};
use crate::widgets::chat::{ChatMessage, ChatWidget};
use crate::widgets::input::InputWidget;
use crate::widgets::status_bar::StatusBarWidget;

/// Application state
pub struct App {
    /// Chat message history for display
    messages: Vec<ChatMessage>,
    /// Canonical message history for the provider
    history: Vec<Message>,
    /// Current input text
    input: String,
    /// Cursor position in input
    cursor_pos: usize,
    /// Text being streamed from the LLM
    streaming_text: String,
    /// Whether we're currently streaming a response
    is_streaming: bool,
    /// Scroll offset for chat area
    scroll_offset: u16,
    /// Total tokens
    input_tokens: u64,
    output_tokens: u64,
    /// Provider (Arc for spawning into tasks)
    provider: Arc<dyn Provider>,
    /// Config
    config: ResolvedConfig,
    /// Should the app exit
    should_quit: bool,
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
        }
    }

    /// Run the main TUI event loop
    pub async fn run(mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        stdout().execute(EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;

        // Event channel
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AppEvent>();
        spawn_event_reader(event_tx);

        // Agent response channel
        let (agent_tx, mut agent_rx) = mpsc::channel::<StreamEvent>(256);

        // Welcome message
        self.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: format!(
                "Hello! I'm lukan, powered by {} / {}. How can I help?",
                self.provider.name(),
                self.config.effective_model()
            ),
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

                // Chat
                let chat =
                    ChatWidget::new(&self.messages, &self.streaming_text, self.scroll_offset);
                frame.render_widget(chat, chunks[0]);

                // Input
                let input_widget =
                    InputWidget::new(&self.input, self.cursor_pos, !self.is_streaming);
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

                // Set cursor position
                if !self.is_streaming {
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
                            if is_quit(&key) {
                                self.should_quit = true;
                            } else if !self.is_streaming {
                                match key.code {
                                    KeyCode::Enter => {
                                        if !self.input.trim().is_empty() {
                                            self.submit_message(agent_tx.clone());
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
                                    KeyCode::Home => {
                                        self.cursor_pos = 0;
                                    }
                                    KeyCode::End => {
                                        self.cursor_pos = self.input.len();
                                    }
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

        // Restore terminal
        disable_raw_mode()?;
        stdout().execute(LeaveAlternateScreen)?;

        Ok(())
    }

    /// Submit the current input as a user message and spawn a streaming task
    fn submit_message(&mut self, agent_tx: mpsc::Sender<StreamEvent>) {
        let text = self.input.trim().to_string();
        self.input.clear();
        self.cursor_pos = 0;

        // Add to display
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: text.clone(),
        });

        // Add to history
        self.history.push(Message::user(&text));

        // Start streaming
        self.is_streaming = true;
        self.streaming_text.clear();

        // Build params
        let system_prompt =
            SystemPrompt::Text(include_str!("../../../prompts/base.txt").to_string());

        let params = StreamParams {
            system_prompt,
            messages: self.history.clone(),
            tools: Vec::new(),
        };

        // Spawn streaming task with Arc<dyn Provider>
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

    /// Handle a streaming event from the provider
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
            _ => {} // Other events handled in Phase 2+
        }
    }
}
