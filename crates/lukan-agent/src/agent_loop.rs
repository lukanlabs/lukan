use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use lukan_core::config::LukanPaths;
use lukan_core::models::events::{StopReason, StreamEvent};
use lukan_core::models::messages::{ContentBlock, Message, MessageContent, Role};
use lukan_core::models::sessions::ChatSession;
use lukan_providers::{Provider, StreamParams, SystemPrompt};
use lukan_tools::{ToolContext, ToolRegistry};
use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, info, warn};

use crate::message_history::MessageHistory;
use crate::session_manager::SessionManager;

// ── Thresholds ────────────────────────────────────────────────────────────

/// When context tokens reach this, trigger MEMORY.md update
const MEMORY_UPDATE_THRESHOLD: u64 = 50_000;
/// When context tokens reach this, trigger auto-compaction
const COMPACTION_THRESHOLD: u64 = 150_000;
/// Keep last N messages during compaction; summarize everything before
const COMPACTION_KEEP_MESSAGES: usize = 10;

// ── Prompts (embedded at compile time) ────────────────────────────────────

const COMPACTION_SIMPLE_PROMPT: &str = include_str!("../../../prompts/compaction-simple.txt");
const COMPACTION_WITH_MEMORY_PROMPT: &str =
    include_str!("../../../prompts/compaction-with-memory.txt");
const MEMORY_UPDATE_PROMPT: &str = include_str!("../../../prompts/memory_update.txt");

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
    /// Last context size (input tokens from most recent LLM call)
    last_context_size: u64,
    /// Tokens at last memory update
    last_memory_update_tokens: u64,
    read_files: Arc<Mutex<HashSet<PathBuf>>>,
}

impl AgentLoop {
    /// Create a new agent with a fresh session
    pub async fn new(mut config: AgentConfig) -> Result<Self> {
        // Register sub-agent tools
        config
            .tools
            .register(Box::new(crate::sub_agent::SubAgentTool));
        config
            .tools
            .register(Box::new(crate::sub_agent::SubAgentResultTool));

        // Configure the global sub-agent manager
        crate::sub_agent::configure(
            Arc::clone(&config.provider),
            config.system_prompt.clone(),
            config.cwd.clone(),
            config.provider_name.clone(),
            config.model_name.clone(),
        );

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
            last_context_size: 0,
            last_memory_update_tokens: 0,
            read_files: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    /// Load an existing session and restore history
    pub async fn load_session(mut config: AgentConfig, session_id: &str) -> Result<Self> {
        // Register sub-agent tools
        config
            .tools
            .register(Box::new(crate::sub_agent::SubAgentTool));
        config
            .tools
            .register(Box::new(crate::sub_agent::SubAgentResultTool));

        crate::sub_agent::configure(
            Arc::clone(&config.provider),
            config.system_prompt.clone(),
            config.cwd.clone(),
            config.provider_name.clone(),
            config.model_name.clone(),
        );

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
            last_context_size: session.last_context_size,
            last_memory_update_tokens: session.last_memory_update_tokens,
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
                        // Track last context size for compaction decisions
                        self.last_context_size = *input_tokens;
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
                role: Role::User,
                content: MessageContent::Blocks(result_blocks),
                tool_call_id: None,
                name: None,
            });

            // Loop continues — LLM will see the tool results and decide next action
        }

        // Auto-save session after each turn
        self.save_session().await?;

        // Check if we need compaction or memory update (non-blocking)
        self.check_auto_ops(&event_tx).await;

        Ok(())
    }

    /// Save current state to disk
    async fn save_session(&mut self) -> Result<()> {
        self.session.messages = self.history.to_json();
        self.session.total_input_tokens = self.input_tokens;
        self.session.total_output_tokens = self.output_tokens;
        self.session.last_context_size = self.last_context_size;
        self.session.last_memory_update_tokens = self.last_memory_update_tokens;
        self.session.updated_at = Utc::now();
        SessionManager::save(&mut self.session).await
    }

    // ── Auto Operations ───────────────────────────────────────────────────

    /// Check if compaction or memory update is needed
    async fn check_auto_ops(&mut self, event_tx: &mpsc::Sender<StreamEvent>) {
        let ctx = self.last_context_size;

        // Auto-compaction at 150k context tokens
        if ctx >= COMPACTION_THRESHOLD {
            if let Err(e) = self.compact_history(event_tx).await {
                error!("Compaction failed: {e}");
            }
            return;
        }

        // Memory update at 50k context tokens
        if ctx >= MEMORY_UPDATE_THRESHOLD {
            let total_used = self.input_tokens + self.output_tokens;
            if total_used - self.last_memory_update_tokens >= MEMORY_UPDATE_THRESHOLD
                && let Err(e) = self.update_memory().await
            {
                error!("Memory update failed: {e}");
            }
        }
    }

    // ── Compaction ────────────────────────────────────────────────────────

    /// Compact history: summarize old messages, keep last N
    async fn compact_history(&mut self, event_tx: &mpsc::Sender<StreamEvent>) -> Result<()> {
        let messages = self.history.messages();
        if messages.len() <= COMPACTION_KEEP_MESSAGES {
            return Ok(());
        }

        // Notify TUI
        let _ = event_tx
            .send(StreamEvent::ToolProgress {
                id: String::new(),
                name: "system".to_string(),
                content: "Compacting conversation...".to_string(),
            })
            .await;

        let msg_count_before = messages.len();
        let split = messages.len() - COMPACTION_KEEP_MESSAGES;
        let old_messages = &messages[..split];
        let recent_messages = &messages[split..];

        let old_context = format_messages_for_context(old_messages);
        let recent_context = format_messages_for_context(recent_messages);

        let full_context = format!(
            "--- OLDER MESSAGES (to be summarized) ---\n{old_context}\n\n\
             --- RECENT MESSAGES (still in context, shown for reference) ---\n{recent_context}"
        );

        // Check if MEMORY.md exists
        let memory_path = LukanPaths::global_memory_file();
        let memory_active = tokio::fs::metadata(&memory_path).await.is_ok();

        let summary;

        if memory_active {
            let current_memory = tokio::fs::read_to_string(&memory_path)
                .await
                .unwrap_or_else(|_| "# Project Memory\n\n".to_string());

            let user_prompt = format!(
                "Current MEMORY.md:\n```\n{current_memory}\n```\n\nConversation to summarize:\n{full_context}"
            );

            let result = self
                .call_llm_for_memory(COMPACTION_WITH_MEMORY_PROMPT, &user_prompt)
                .await?;

            // Parse ---SUMMARY--- and ---MEMORY--- sections
            let summary_match = extract_section(&result, "---SUMMARY---", "---MEMORY---");
            let memory_match = extract_section(&result, "---MEMORY---", "");

            summary = summary_match
                .unwrap_or_else(|| "Previous conversation context was compacted.".to_string());

            if let Some(updated_memory) = memory_match {
                write_memory_file(&updated_memory).await;
            }
        } else {
            summary = self
                .call_llm_for_memory(COMPACTION_SIMPLE_PROMPT, &full_context)
                .await
                .unwrap_or_else(|_| "Previous conversation context was compacted.".to_string());
        }

        // Rebuild history: compaction message + recent messages
        let compaction_msg = format!(
            "[System: Conversation was auto-compacted. Below is a summary of earlier context. \
             Continue working from where you left off — check the \"Active Task\" section for what you were doing.]\n\n\
             {summary}"
        );

        let recent = self.history.messages()[split..].to_vec();
        self.history.clear();
        self.history.add_user_message(&compaction_msg);
        for msg in recent {
            self.history.add(msg);
        }

        // Reset token counters
        self.session.compaction_count += 1;
        self.session.compaction_summary = Some(summary);
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.last_context_size = 0;
        self.last_memory_update_tokens = 0;

        self.save_session().await?;

        let msg_count_after = self.history.messages().len();
        info!(
            before = msg_count_before,
            after = msg_count_after,
            "Compacted history"
        );

        let _ = event_tx
            .send(StreamEvent::ToolProgress {
                id: String::new(),
                name: "system".to_string(),
                content: format!("Compacted: {msg_count_before} msgs → {msg_count_after} msgs."),
            })
            .await;

        Ok(())
    }

    // ── Memory Update ─────────────────────────────────────────────────────

    /// Update MEMORY.md with insights from the conversation
    async fn update_memory(&mut self) -> Result<()> {
        let messages = self.history.messages();
        let context = format_messages_for_context(messages);

        let memory_path = LukanPaths::global_memory_file();
        let current_memory = tokio::fs::read_to_string(&memory_path)
            .await
            .unwrap_or_else(|_| "# Project Memory\n\n".to_string());

        let user_prompt = format!(
            "Current MEMORY.md:\n```\n{current_memory}\n```\n\n\
             Conversation:\n{context}\n\n\
             Analyze the conversation and update MEMORY.md. \
             Preserve existing useful information, add new patterns/decisions/solutions \
             discovered in the conversation. Remove outdated information. \
             Output only the updated markdown content."
        );

        let updated = self
            .call_llm_for_memory(MEMORY_UPDATE_PROMPT, &user_prompt)
            .await?;

        if !updated.is_empty() {
            write_memory_file(&updated).await;
        }

        self.last_memory_update_tokens = self.input_tokens + self.output_tokens;
        info!("Updated MEMORY.md");

        Ok(())
    }

    // ── LLM Helper ────────────────────────────────────────────────────────

    /// Make a simple LLM call for compaction/memory (no tools, no streaming to UI)
    async fn call_llm_for_memory(&self, system_prompt: &str, user_message: &str) -> Result<String> {
        let params = StreamParams {
            system_prompt: SystemPrompt::Text(system_prompt.to_string()),
            messages: vec![Message::user(user_message)],
            tools: vec![],
        };

        let (tx, mut rx) = mpsc::channel::<StreamEvent>(256);
        let provider = Arc::clone(&self.provider);

        tokio::spawn(async move {
            if let Err(e) = provider.stream(params, tx).await {
                error!("Memory LLM call error: {e}");
            }
        });

        let mut result = String::new();
        while let Some(event) = rx.recv().await {
            if let StreamEvent::TextDelta { text } = event {
                result.push_str(&text);
            }
        }

        Ok(result)
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

// ── Helpers ───────────────────────────────────────────────────────────────

/// Format messages into a text representation for compaction/memory LLM calls
fn format_messages_for_context(messages: &[Message]) -> String {
    let mut output = String::new();
    for msg in messages {
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::Tool => "Tool",
        };

        match &msg.content {
            MessageContent::Text(text) => {
                output.push_str(&format!("[{role}]: {text}\n\n"));
            }
            MessageContent::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        ContentBlock::Text { text } => {
                            output.push_str(&format!("[{role}]: {text}\n\n"));
                        }
                        ContentBlock::Thinking { text } => {
                            output.push_str(&format!("[{role} thinking]: {text}\n\n"));
                        }
                        ContentBlock::ToolUse { name, input, .. } => {
                            let input_str = serde_json::to_string(input).unwrap_or_default();
                            let truncated = if input_str.len() > 500 {
                                format!("{}...", &input_str[..500])
                            } else {
                                input_str
                            };
                            output.push_str(&format!("[{role} tool_use]: {name}({truncated})\n\n"));
                        }
                        ContentBlock::ToolResult {
                            content, is_error, ..
                        } => {
                            let prefix = if *is_error == Some(true) {
                                "ERROR"
                            } else {
                                "result"
                            };
                            // Truncate long tool results
                            let truncated = if content.len() > 2000 {
                                format!("{}...(truncated)", &content[..2000])
                            } else {
                                content.clone()
                            };
                            output.push_str(&format!("[Tool {prefix}]: {truncated}\n\n"));
                        }
                        ContentBlock::Image { .. } => {
                            output.push_str(&format!("[{role}]: [Image]\n\n"));
                        }
                    }
                }
            }
        }
    }
    output
}

/// Extract a section between two markers from LLM output
fn extract_section(text: &str, start_marker: &str, end_marker: &str) -> Option<String> {
    let start = text.find(start_marker)?;
    let content_start = start + start_marker.len();
    let content = if end_marker.is_empty() {
        &text[content_start..]
    } else if let Some(end) = text[content_start..].find(end_marker) {
        &text[content_start..content_start + end]
    } else {
        &text[content_start..]
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Write MEMORY.md to disk
async fn write_memory_file(content: &str) {
    let path = LukanPaths::global_memory_file();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Err(e) = tokio::fs::write(&path, content).await {
        error!("Failed to write MEMORY.md: {e}");
    }
}
