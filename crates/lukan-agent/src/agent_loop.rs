use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use lukan_core::models::events::{StopReason, StreamEvent};
use lukan_core::models::messages::{ContentBlock, Message, MessageContent};
use lukan_core::models::sessions::ChatSession;
use lukan_providers::{Provider, StreamParams, SystemPrompt};
use lukan_tools::{ToolContext, ToolRegistry};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, warn};

use crate::message_history::MessageHistory;
use crate::session_manager::SessionManager;

/// Configuration for creating an AgentLoop
pub struct AgentConfig {
    pub provider: Arc<dyn Provider>,
    pub tools: ToolRegistry,
    pub system_prompt: SystemPrompt,
    pub cwd: PathBuf,
    /// Provider name for session metadata
    pub provider_name: String,
    /// Model name for session metadata
    pub model_name: String,
}

/// Pending tool call accumulated from stream events
struct PendingToolCall {
    id: String,
    name: String,
    input: serde_json::Value,
}

/// The agent loop that coordinates LLM ↔ Tools
pub struct AgentLoop {
    provider: Arc<dyn Provider>,
    tools: Arc<ToolRegistry>,
    system_prompt: SystemPrompt,
    cwd: PathBuf,
    history: MessageHistory,
    session: ChatSession,
    input_tokens: u64,
    output_tokens: u64,
    read_files: Arc<Mutex<HashSet<PathBuf>>>,
}

impl AgentLoop {
    /// Create a new agent with a fresh session
    pub async fn new(config: AgentConfig) -> Result<Self> {
        let session = SessionManager::create(&config.provider_name, &config.model_name).await?;
        Ok(Self {
            provider: config.provider,
            tools: Arc::new(config.tools),
            system_prompt: config.system_prompt,
            cwd: config.cwd,
            history: MessageHistory::new(),
            session,
            input_tokens: 0,
            output_tokens: 0,
            read_files: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Load an existing session and restore history
    pub async fn load_session(config: AgentConfig, session_id: &str) -> Result<Self> {
        let session = SessionManager::load(session_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Session not found: {session_id}"))?;

        let mut history = MessageHistory::new();
        history.load_from_json(session.messages.clone());

        Ok(Self {
            provider: config.provider,
            tools: Arc::new(config.tools),
            system_prompt: config.system_prompt,
            cwd: config.cwd,
            history,
            input_tokens: session.total_input_tokens,
            output_tokens: session.total_output_tokens,
            session,
            read_files: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session.id
    }

    /// Total input tokens used across all turns
    pub fn input_tokens(&self) -> u64 {
        self.input_tokens
    }

    /// Total output tokens used across all turns
    pub fn output_tokens(&self) -> u64 {
        self.output_tokens
    }

    /// Run a single user turn: sends the message and loops until no more tool calls
    pub async fn run_turn(
        &mut self,
        user_message: &str,
        event_tx: mpsc::Sender<StreamEvent>,
    ) -> Result<()> {
        // Add user message to history
        self.history.add_user_message(user_message);

        // Inner loop: call LLM → execute tools → repeat until done
        loop {
            let tool_defs = self.tools.definitions();

            let params = StreamParams {
                system_prompt: self.system_prompt.clone(),
                messages: self.history.messages().to_vec(),
                tools: tool_defs,
            };

            // Stream from LLM
            let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(256);
            let provider = Arc::clone(&self.provider);

            let stream_handle = tokio::spawn(async move {
                if let Err(e) = provider.stream(params, stream_tx).await {
                    error!("Provider stream error: {e}");
                }
            });

            // Accumulate the response
            let mut text_content = String::new();
            let mut thinking_content = String::new();
            let mut pending_tools: Vec<PendingToolCall> = Vec::new();
            let mut stop_reason = StopReason::EndTurn;

            while let Some(event) = stream_rx.recv().await {
                match &event {
                    StreamEvent::TextDelta { text } => {
                        text_content.push_str(text);
                    }
                    StreamEvent::ThinkingDelta { text } => {
                        thinking_content.push_str(text);
                    }
                    StreamEvent::ToolUseEnd { id, name, input } => {
                        pending_tools.push(PendingToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            input: input.clone(),
                        });
                    }
                    StreamEvent::Usage {
                        input_tokens,
                        output_tokens,
                        ..
                    } => {
                        self.input_tokens += input_tokens;
                        self.output_tokens += output_tokens;
                    }
                    StreamEvent::MessageEnd {
                        stop_reason: reason,
                    } => {
                        stop_reason = reason.clone();
                    }
                    _ => {}
                }

                // Forward all events to the TUI
                if event_tx.send(event).await.is_err() {
                    warn!("Event receiver dropped, aborting turn");
                    stream_handle.abort();
                    return Ok(());
                }
            }

            // Wait for provider task to finish
            let _ = stream_handle.await;

            // Build assistant message with all content blocks
            let mut blocks = Vec::new();

            if !thinking_content.is_empty() {
                blocks.push(ContentBlock::Thinking {
                    text: thinking_content,
                });
            }

            if !text_content.is_empty() {
                blocks.push(ContentBlock::Text {
                    text: text_content.clone(),
                });
            }

            for tool in &pending_tools {
                blocks.push(ContentBlock::ToolUse {
                    id: tool.id.clone(),
                    name: tool.name.clone(),
                    input: tool.input.clone(),
                });
            }

            if !blocks.is_empty() {
                self.history.add_assistant_blocks(blocks);
            }

            // If no tool calls, we're done
            if stop_reason != StopReason::ToolUse || pending_tools.is_empty() {
                debug!(
                    stop_reason = ?stop_reason,
                    "Turn complete, no tool calls"
                );
                break;
            }

            // Execute tool calls in parallel
            debug!(count = pending_tools.len(), "Executing tool calls");

            let tool_results = self.execute_tools(&pending_tools, &event_tx).await;

            // Add tool results to history and forward events
            let mut result_blocks = Vec::new();
            for (tool, result) in pending_tools.iter().zip(tool_results.iter()) {
                result_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: tool.id.clone(),
                    content: result.content.clone(),
                    is_error: if result.is_error { Some(true) } else { None },
                    diff: result.diff.clone(),
                    image: result.image.clone(),
                });

                // Send ToolResult event to TUI
                let _ = event_tx
                    .send(StreamEvent::ToolResult {
                        id: tool.id.clone(),
                        name: tool.name.clone(),
                        content: result.content.clone(),
                        is_error: if result.is_error { Some(true) } else { None },
                        diff: result.diff.clone(),
                        image: result.image.clone(),
                    })
                    .await;
            }

            self.history.add(Message {
                role: lukan_core::models::messages::Role::User,
                content: MessageContent::Blocks(result_blocks),
                tool_call_id: None,
                name: None,
            });

            // Loop continues — LLM will see the tool results and decide next action
        }

        // Auto-save session after each turn
        self.save_session().await?;

        Ok(())
    }

    /// Save current state to disk
    async fn save_session(&mut self) -> Result<()> {
        self.session.messages = self.history.to_json();
        self.session.total_input_tokens = self.input_tokens;
        self.session.total_output_tokens = self.output_tokens;
        self.session.updated_at = Utc::now();
        SessionManager::save(&mut self.session).await
    }

    /// Execute multiple tool calls in parallel
    async fn execute_tools(
        &self,
        tools: &[PendingToolCall],
        event_tx: &mpsc::Sender<StreamEvent>,
    ) -> Vec<lukan_core::models::tools::ToolResult> {
        let mut handles = Vec::new();

        for tool_call in tools {
            let registry = Arc::clone(&self.tools);
            let read_files = Arc::clone(&self.read_files);
            let cwd = self.cwd.clone();
            let name = tool_call.name.clone();
            let id = tool_call.id.clone();
            let input = tool_call.input.clone();
            let tx = event_tx.clone();

            handles.push(tokio::spawn(async move {
                // Send progress start
                let _ = tx
                    .send(StreamEvent::ToolProgress {
                        id: id.clone(),
                        name: name.clone(),
                        content: format!("Running {name}..."),
                    })
                    .await;

                let ctx = ToolContext {
                    progress_tx: None,
                    read_files,
                    cwd,
                };

                match registry.execute(&name, input, &ctx).await {
                    Ok(result) => result,
                    Err(e) => {
                        error!(tool = name, error = %e, "Tool execution failed");
                        lukan_core::models::tools::ToolResult::error(format!(
                            "Tool execution error: {e}"
                        ))
                    }
                }
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => {
                    results.push(lukan_core::models::tools::ToolResult::error(format!(
                        "Task join error: {e}"
                    )));
                }
            }
        }

        results
    }
}
