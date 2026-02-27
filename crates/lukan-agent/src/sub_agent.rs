//! Sub-agent system: spawn autonomous child agents for parallel work.
//!
//! - `SubAgentTool` spawns a sub-agent in the background
//! - `SubAgentResultTool` checks or waits for a sub-agent's result
//! - `SubAgentManager` tracks running/completed sub-agents

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lukan_core::models::events::{StopReason, StreamEvent};
use lukan_core::models::tools::ToolResult;
use lukan_providers::{Provider, StreamParams, SystemPrompt};
use lukan_tools::{Tool, ToolContext, create_default_registry};
use serde_json::json;
use tokio::sync::{Mutex, RwLock, mpsc};
use tracing::{error, info};

use crate::message_history::MessageHistory;

// ── Global Manager ────────────────────────────────────────────────────────

static MANAGER: std::sync::LazyLock<RwLock<SubAgentManager>> =
    std::sync::LazyLock::new(|| RwLock::new(SubAgentManager::new()));

/// Configure the sub-agent manager with the parent's provider info
pub async fn configure(
    provider: Arc<dyn Provider>,
    system_prompt: SystemPrompt,
    cwd: std::path::PathBuf,
    provider_name: String,
    model_name: String,
    sandbox: Option<lukan_tools::sandbox::SandboxConfig>,
    allowed_paths: Option<Vec<std::path::PathBuf>>,
) {
    let mut mgr = MANAGER.write().await;
    mgr.provider = Some(provider);
    mgr.system_prompt = Some(system_prompt);
    mgr.cwd = Some(cwd);
    mgr.provider_name = Some(provider_name);
    mgr.model_name = Some(model_name);
    mgr.sandbox = sandbox;
    mgr.allowed_paths = allowed_paths;
}

// ── Manager ───────────────────────────────────────────────────────────────

/// Real-time update pushed from a running sub-agent to the TUI
#[derive(Debug, Clone)]
pub struct SubAgentUpdate {
    pub id: String,
    pub chat_messages: Vec<SubAgentChatMsg>,
    pub status: String,
    pub turns: usize,
    pub max_turns: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub error: Option<String>,
}

struct SubAgentManager {
    entries: HashMap<String, SubAgentEntry>,
    provider: Option<Arc<dyn Provider>>,
    system_prompt: Option<SystemPrompt>,
    cwd: Option<std::path::PathBuf>,
    provider_name: Option<String>,
    model_name: Option<String>,
    sandbox: Option<lukan_tools::sandbox::SandboxConfig>,
    allowed_paths: Option<Vec<std::path::PathBuf>>,
    /// Channel to push real-time updates to the TUI
    update_tx: Option<mpsc::Sender<SubAgentUpdate>>,
}

impl SubAgentManager {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            provider: None,
            system_prompt: None,
            cwd: None,
            provider_name: None,
            model_name: None,
            sandbox: None,
            allowed_paths: None,
            update_tx: None,
        }
    }
}

/// Subscribe to real-time sub-agent updates. Returns a receiver.
/// Only one subscriber at a time (the TUI).
pub async fn subscribe_updates() -> mpsc::Receiver<SubAgentUpdate> {
    let (tx, rx) = mpsc::channel(64);
    let mut mgr = MANAGER.write().await;
    mgr.update_tx = Some(tx);
    rx
}

/// A chat message from the sub-agent conversation (for spectator view)
#[derive(Debug, Clone)]
pub struct SubAgentChatMsg {
    pub role: String,
    pub content: String,
}

/// Tracks a sub-agent's lifecycle
#[derive(Debug, Clone)]
pub struct SubAgentEntry {
    pub id: String,
    pub task: String,
    pub status: SubAgentStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub turns: usize,
    pub max_turns: usize,
    pub text_output: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub error: Option<String>,
    /// Full chat conversation for the spectator view
    pub chat_messages: Vec<SubAgentChatMsg>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubAgentStatus {
    Running,
    Completed,
    Error,
    Aborted,
}

impl std::fmt::Display for SubAgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Completed => write!(f, "completed"),
            Self::Error => write!(f, "error"),
            Self::Aborted => write!(f, "aborted"),
        }
    }
}

// ── Spawn / Query ─────────────────────────────────────────────────────────

async fn spawn_sub_agent(
    task: String,
    timeout_ms: u64,
    max_turns: usize,
) -> anyhow::Result<String> {
    let id = {
        let mut buf = [0u8; 3];
        use rand::Rng;
        rand::rng().fill(&mut buf);
        hex::encode(&buf)
    };

    let (provider, system_prompt, cwd, _provider_name, _model_name, sandbox, allowed_paths) = {
        let mgr = MANAGER.read().await;
        let provider = mgr
            .provider
            .clone()
            .ok_or_else(|| anyhow::anyhow!("SubAgent manager not configured"))?;
        let sp = mgr
            .system_prompt
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No system prompt configured"))?;
        let cwd = mgr.cwd.clone().unwrap_or_else(|| "/tmp".into());
        let pn = mgr.provider_name.clone().unwrap_or_default();
        let mn = mgr.model_name.clone().unwrap_or_default();
        let sandbox = mgr.sandbox.clone();
        let allowed_paths = mgr.allowed_paths.clone();
        (provider, sp, cwd, pn, mn, sandbox, allowed_paths)
    };

    let entry = SubAgentEntry {
        id: id.clone(),
        task: task.clone(),
        status: SubAgentStatus::Running,
        started_at: Utc::now(),
        completed_at: None,
        turns: 0,
        max_turns,
        text_output: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        error: None,
        chat_messages: Vec::new(),
    };

    {
        let mut mgr = MANAGER.write().await;
        mgr.entries.insert(id.clone(), entry);
    }

    let agent_id = id.clone();
    tokio::spawn(async move {
        run_sub_agent(
            agent_id,
            task,
            timeout_ms,
            max_turns,
            provider,
            system_prompt,
            cwd,
            sandbox,
            allowed_paths,
        )
        .await;
    });

    Ok(id)
}

#[allow(clippy::too_many_arguments)]
async fn run_sub_agent(
    id: String,
    task: String,
    timeout_ms: u64,
    max_turns: usize,
    provider: Arc<dyn Provider>,
    system_prompt: SystemPrompt,
    cwd: std::path::PathBuf,
    sandbox: Option<lukan_tools::sandbox::SandboxConfig>,
    allowed_paths: Option<Vec<std::path::PathBuf>>,
) {
    let mut history = MessageHistory::new();
    history.add_user_message(&task);

    // Get the update channel sender (if TUI is subscribed)
    let update_tx = {
        let mgr = MANAGER.read().await;
        mgr.update_tx.clone()
    };

    // Create tools but remove SubAgent/SubAgentResult to prevent recursion
    let tools = create_default_registry();
    // Tools we want to remove (they call themselves)
    // ToolRegistry doesn't have unregister, so we recreate without them
    // Actually, since SubAgent tools are registered by AgentLoop, not create_default_registry,
    // the sub-agent's registry from create_default_registry won't have them. This is fine.

    let tools = Arc::new(tools);
    let read_files = Arc::new(Mutex::new(std::collections::HashSet::new()));

    let mut turns = 0;
    let mut text_output = String::new();
    let mut chat_messages: Vec<SubAgentChatMsg> = Vec::new();
    // Add the initial user task as the first chat message
    chat_messages.push(SubAgentChatMsg {
        role: "user".to_string(),
        content: task.clone(),
    });
    let mut total_input = 0u64;
    let mut total_output = 0u64;
    let mut final_status = SubAgentStatus::Completed;
    let mut final_error = None;

    let timeout = tokio::time::sleep(std::time::Duration::from_millis(timeout_ms));
    tokio::pin!(timeout);

    'outer: loop {
        if turns >= max_turns {
            final_status = SubAgentStatus::Aborted;
            text_output.push_str("\n[Reached maximum turns]");
            break;
        }

        let params = StreamParams {
            system_prompt: system_prompt.clone(),
            messages: history.messages().to_vec(),
            tools: tools.definitions(),
        };

        let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(256);
        let prov = Arc::clone(&provider);

        let stream_handle = tokio::spawn(async move {
            if let Err(e) = prov.stream(params, stream_tx).await {
                error!("SubAgent stream error: {e}");
            }
        });

        let mut text_content = String::new();
        let mut thinking_content = String::new();
        let mut pending_tools = Vec::new();
        let mut stop_reason = StopReason::EndTurn;

        loop {
            tokio::select! {
                event = stream_rx.recv() => {
                    let Some(event) = event else { break };
                    match &event {
                        StreamEvent::TextDelta { text } => text_content.push_str(text),
                        StreamEvent::ThinkingDelta { text } => thinking_content.push_str(text),
                        StreamEvent::ToolUseEnd { id, name, input } => {
                            pending_tools.push((id.clone(), name.clone(), input.clone()));
                        }
                        StreamEvent::Usage { input_tokens, output_tokens, .. } => {
                            total_input += input_tokens;
                            total_output += output_tokens;
                        }
                        StreamEvent::MessageEnd { stop_reason: r } => {
                            stop_reason = r.clone();
                        }
                        StreamEvent::Error { error } => {
                            final_error = Some(error.clone());
                            final_status = SubAgentStatus::Error;
                            break 'outer;
                        }
                        _ => {}
                    }
                }
                _ = &mut timeout => {
                    stream_handle.abort();
                    final_status = SubAgentStatus::Aborted;
                    text_output.push_str("\n[Timeout]");
                    break 'outer;
                }
            }
        }

        let _ = stream_handle.await;

        // Accumulate text output
        if !text_content.is_empty() {
            text_output.push_str(&text_content);
            chat_messages.push(SubAgentChatMsg {
                role: "assistant".to_string(),
                content: text_content.clone(),
            });
        }

        // Log tool calls as chat messages
        for (_tool_id, name, input) in &pending_tools {
            let arg = get_display_arg(name, input);
            chat_messages.push(SubAgentChatMsg {
                role: "tool_call".to_string(),
                content: format!("● {name}({arg})"),
            });
        }

        // Build assistant blocks
        let mut blocks = Vec::new();
        if !thinking_content.is_empty() {
            blocks.push(lukan_core::models::messages::ContentBlock::Thinking {
                text: thinking_content,
            });
        }
        if !text_content.is_empty() {
            blocks.push(lukan_core::models::messages::ContentBlock::Text { text: text_content });
        }
        for (tool_id, name, input) in &pending_tools {
            blocks.push(lukan_core::models::messages::ContentBlock::ToolUse {
                id: tool_id.clone(),
                name: name.clone(),
                input: input.clone(),
            });
        }
        if !blocks.is_empty() {
            history.add_assistant_blocks(blocks);
        }

        turns += 1;

        // Update manager with progress
        {
            let mut mgr = MANAGER.write().await;
            if let Some(entry) = mgr.entries.get_mut(&id) {
                entry.turns = turns;
                entry.text_output = text_output.clone();
                entry.chat_messages = chat_messages.clone();
                entry.input_tokens = total_input;
                entry.output_tokens = total_output;
            }
        }

        // Push real-time update to TUI
        if let Some(ref tx) = update_tx {
            let _ = tx.try_send(SubAgentUpdate {
                id: id.clone(),
                chat_messages: chat_messages.clone(),
                status: "running".to_string(),
                turns,
                max_turns,
                input_tokens: total_input,
                output_tokens: total_output,
                error: None,
            });
        }

        if stop_reason != StopReason::ToolUse || pending_tools.is_empty() {
            break;
        }

        // Execute tools in parallel
        let mut handles = Vec::new();
        for (_tool_id, name, input) in &pending_tools {
            let reg = Arc::clone(&tools);
            let rf = Arc::clone(&read_files);
            let c = cwd.clone();
            let n = name.clone();
            let inp = input.clone();

            let sandbox_cfg = sandbox.clone();
            let ap = allowed_paths.clone();
            handles.push(tokio::spawn(async move {
                let ctx = ToolContext {
                    progress_tx: None,
                    event_tx: None,
                    tool_call_id: None,
                    read_files: rf,
                    cwd: c,
                    bg_signal: None,
                    sandbox: sandbox_cfg,
                    allowed_paths: ap,
                    cancel: None,
                };
                match reg.execute(&n, inp, &ctx).await {
                    Ok(r) => r,
                    Err(e) => ToolResult::error(format!("Tool error: {e}")),
                }
            }));
        }

        let mut result_blocks = Vec::new();
        for (i, handle) in handles.into_iter().enumerate() {
            let result = match handle.await {
                Ok(r) => r,
                Err(e) => ToolResult::error(format!("Join error: {e}")),
            };
            let (tool_id, tool_name, _) = &pending_tools[i];

            // Log tool result as chat message
            let summary = summarize_result(tool_name, &result.content, result.is_error);
            chat_messages.push(SubAgentChatMsg {
                role: "tool_result".to_string(),
                content: format!("  ⎿  {summary}"),
            });

            result_blocks.push(lukan_core::models::messages::ContentBlock::ToolResult {
                tool_use_id: tool_id.clone(),
                content: result.content.clone(),
                is_error: if result.is_error { Some(true) } else { None },
                diff: result.diff,
                image: result.image,
            });
        }

        // Update chat messages after tool results
        {
            let mut mgr = MANAGER.write().await;
            if let Some(entry) = mgr.entries.get_mut(&id) {
                entry.chat_messages = chat_messages.clone();
            }
        }

        // Push tool results update to TUI
        if let Some(ref tx) = update_tx {
            let _ = tx.try_send(SubAgentUpdate {
                id: id.clone(),
                chat_messages: chat_messages.clone(),
                status: "running".to_string(),
                turns,
                max_turns,
                input_tokens: total_input,
                output_tokens: total_output,
                error: None,
            });
        }

        history.add(lukan_core::models::messages::Message {
            role: lukan_core::models::messages::Role::User,
            content: lukan_core::models::messages::MessageContent::Blocks(result_blocks),
            tool_call_id: None,
            name: None,
        });
    }

    // Finalize
    let final_status_str = format!("{final_status}");
    {
        let mut mgr = MANAGER.write().await;
        if let Some(entry) = mgr.entries.get_mut(&id) {
            entry.status = final_status;
            entry.completed_at = Some(Utc::now());
            entry.turns = turns;
            entry.text_output = text_output;
            entry.chat_messages = chat_messages.clone();
            entry.input_tokens = total_input;
            entry.output_tokens = total_output;
            entry.error = final_error.clone();
        }
    }

    // Push final update to TUI
    if let Some(ref tx) = update_tx {
        let _ = tx.try_send(SubAgentUpdate {
            id: id.clone(),
            chat_messages,
            status: final_status_str,
            turns,
            max_turns,
            input_tokens: total_input,
            output_tokens: total_output,
            error: final_error,
        });
    }

    info!(id, turns, "Sub-agent completed");
}

async fn get_sub_agent(id: &str) -> Option<SubAgentEntry> {
    let mgr = MANAGER.read().await;
    mgr.entries.get(id).cloned()
}

/// Get all sub-agent entries (for UI display)
pub async fn get_all_sub_agents() -> Vec<SubAgentEntry> {
    let mgr = MANAGER.read().await;
    mgr.entries.values().cloned().collect()
}

// ── hex module (inline) ──────────────────────────────────────────────────
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}

// ── SubAgent Tool ─────────────────────────────────────────────────────────

pub struct SubAgentTool;

#[async_trait]
impl Tool for SubAgentTool {
    fn name(&self) -> &str {
        "SubAgent"
    }

    fn description(&self) -> &str {
        "Spawn an autonomous sub-agent to handle a task in the background. \
         The sub-agent runs independently with its own conversation and tools. \
         Use SubAgentResult to check status or get results."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The task for the sub-agent to perform autonomously"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 120000)",
                    "default": 120000
                },
                "maxTurns": {
                    "type": "integer",
                    "description": "Maximum LLM turns before stopping (default: 20)",
                    "default": 20
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let task = input
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: task"))?
            .to_string();

        let timeout = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);

        let max_turns = input.get("maxTurns").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

        match spawn_sub_agent(task.clone(), timeout, max_turns).await {
            Ok(id) => Ok(ToolResult::success(format!(
                "Sub-agent spawned (ID: {id})\nTask: {task}\n\n\
                 Running in background. Use SubAgentResult(\"{id}\") to check status/results."
            ))),
            Err(e) => Ok(ToolResult::error(format!("SubAgent error: {e}"))),
        }
    }
}

// ── SubAgentResult Tool ───────────────────────────────────────────────────

pub struct SubAgentResultTool;

#[async_trait]
impl Tool for SubAgentResultTool {
    fn name(&self) -> &str {
        "SubAgentResult"
    }

    fn description(&self) -> &str {
        "Check the status or get results from a sub-agent. \
         Use wait=true to block until the sub-agent completes."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "agentId": {
                    "type": "string",
                    "description": "Sub-agent ID returned by SubAgent tool"
                },
                "wait": {
                    "type": "boolean",
                    "description": "Block until the sub-agent completes (default: false)",
                    "default": false
                }
            },
            "required": ["agentId"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let agent_id = input
            .get("agentId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: agentId"))?;

        let wait = input.get("wait").and_then(|v| v.as_bool()).unwrap_or(false);

        // If waiting, poll until done
        if wait {
            loop {
                let entry = get_sub_agent(agent_id).await;
                match entry {
                    None => {
                        return Ok(ToolResult::error(format!(
                            "Sub-agent \"{agent_id}\" not found."
                        )));
                    }
                    Some(e) if e.status != SubAgentStatus::Running => {
                        return Ok(format_sub_agent_result(&e));
                    }
                    _ => {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                }
            }
        }

        match get_sub_agent(agent_id).await {
            Some(entry) => Ok(format_sub_agent_result(&entry)),
            None => Ok(ToolResult::error(format!(
                "Sub-agent \"{agent_id}\" not found."
            ))),
        }
    }
}

// ── Explore Sub-Agent ──────────────────────────────────────────────────────

const EXPLORE_SYSTEM_PROMPT: &str = "\
You are a codebase research agent. Your ONLY job is to explore code and return detailed findings.
You CANNOT modify files. Use ReadFile, Grep, and Glob to investigate.
Be thorough — read all relevant files, trace call chains, check types and interfaces.
Your output will be used by the main agent to make code changes, so include:
- Exact file paths and line numbers
- Relevant code snippets
- How components connect to each other
- Any patterns or conventions you notice

Guidelines:
- Start with Grep or Glob to locate relevant code, then ReadFile to read specific sections.
- Use ReadFile with offset/limit to read only the relevant parts of large files.
- Call multiple independent tools in parallel when possible.
- Be concise but complete — include everything the main agent needs to act.";

/// Read-only tools available to the Explore sub-agent
const EXPLORE_TOOLS: &[&str] = &["ReadFiles", "Grep", "Glob", "WebFetch"];

/// Extract the main display arg for a tool call (file path, pattern, etc.)
fn get_display_arg(name: &str, input: &serde_json::Value) -> String {
    let s = |key: &str| {
        input
            .get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    match name {
        "ReadFiles" => s("file_path"),
        "Grep" => s("pattern"),
        "Glob" => s("pattern"),
        "WebFetch" => s("url"),
        _ => {
            let j = input.to_string();
            if j.len() > 60 { j[..60].to_string() } else { j }
        }
    }
}

/// Summarize a tool result into a short one-liner
fn summarize_result(name: &str, content: &str, is_error: bool) -> String {
    if is_error {
        let first_line = content.lines().next().unwrap_or("");
        return format!("Error: {}", &first_line[..first_line.len().min(80)]);
    }
    match name {
        "ReadFiles" => {
            let lines = content.lines().filter(|l| !l.trim().is_empty()).count();
            format!("{lines} lines")
        }
        "Grep" => {
            if content == "No matches found." {
                return content.to_string();
            }
            let lines = content.lines().filter(|l| !l.trim().is_empty()).count();
            format!("{lines} result{}", if lines != 1 { "s" } else { "" })
        }
        "Glob" => {
            if content.starts_with("No files") {
                return content.to_string();
            }
            let lines = content.lines().filter(|l| !l.trim().is_empty()).count();
            format!("{lines} file{}", if lines != 1 { "s" } else { "" })
        }
        _ => {
            let first = content.lines().next().unwrap_or("(empty)").trim();
            if first.len() > 60 {
                format!("{}…", &first[..60])
            } else {
                first.to_string()
            }
        }
    }
}

/// Build the full activity display: completed tool lines + in-flight tools + summary
fn build_explore_activity(
    completed_lines: &[String],
    active_tools: &HashMap<String, (String, String)>,
    tool_call_count: u32,
    tokens: u64,
    elapsed_secs: u64,
) -> String {
    let mut lines = Vec::new();

    // Completed tool lines
    for line in completed_lines {
        lines.push(format!("  ⎿  {line}"));
    }

    // In-flight tools (with … to show they're running)
    for (name, arg) in active_tools.values() {
        lines.push(format!("  ⎿  {name}({arg})…"));
    }

    // Summary line
    let tokens_k = tokens as f64 / 1000.0;
    lines.push(format!(
        "  ⎿  {tool_call_count} tool uses · {tokens_k:.1}k tokens · {elapsed_secs}s"
    ));

    lines.join("\n")
}

/// Run an Explore sub-agent synchronously (blocks until done).
///
/// Uses a filtered tool registry with only read-only tools and a
/// research-focused system prompt. Emits `ExploreProgress` events
/// via `progress_tx` for TUI display.
#[allow(clippy::too_many_arguments)]
pub async fn run_explore(
    task: &str,
    timeout_ms: u64,
    max_turns: usize,
    progress_tx: Option<mpsc::Sender<StreamEvent>>,
    explore_id: String,
) -> anyhow::Result<String> {
    let (provider, _system_prompt, cwd, sandbox, allowed_paths) = {
        let mgr = MANAGER.read().await;
        let provider = mgr
            .provider
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Explore: agent not configured yet"))?;
        let sp = mgr
            .system_prompt
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Explore: no system prompt configured"))?;
        let cwd = mgr.cwd.clone().unwrap_or_else(|| "/tmp".into());
        let sandbox = mgr.sandbox.clone();
        let allowed_paths = mgr.allowed_paths.clone();
        (provider, sp, cwd, sandbox, allowed_paths)
    };

    // Use a research-focused system prompt instead of the parent's
    let system_prompt = SystemPrompt::Structured {
        cached: vec![EXPLORE_SYSTEM_PROMPT.to_string()],
        dynamic: String::new(),
    };

    let mut history = MessageHistory::new();
    history.add_user_message(task);

    // Create a filtered tool registry with only read-only tools
    let mut tools = lukan_tools::create_default_registry();
    tools.retain(EXPLORE_TOOLS);
    let tools = Arc::new(tools);

    let read_files = Arc::new(Mutex::new(std::collections::HashSet::new()));

    let mut turns = 0usize;
    let mut text_output = String::new();
    let mut tool_results_fallback: Vec<String> = Vec::new();
    let mut total_input = 0u64;
    let mut total_output = 0u64;
    let mut tool_call_count = 0u32;
    let mut active_tools: HashMap<String, (String, String)> = HashMap::new();
    let mut completed_lines: Vec<String> = Vec::new();
    let started_at = std::time::Instant::now();

    // Use a deadline so the timeout covers both streaming AND tool execution
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);

    let task_display = task.to_string();

    'outer: loop {
        if turns >= max_turns {
            text_output.push_str("\n[Reached maximum turns]");
            break;
        }

        // Check deadline before starting a new turn
        if tokio::time::Instant::now() >= deadline {
            text_output.push_str("\n[Timeout]");
            break;
        }

        let params = StreamParams {
            system_prompt: system_prompt.clone(),
            messages: history.messages().to_vec(),
            tools: tools.definitions(),
        };

        let (stream_tx, mut stream_rx) = mpsc::channel::<StreamEvent>(256);
        let prov = Arc::clone(&provider);

        let stream_handle = tokio::spawn(async move {
            if let Err(e) = prov.stream(params, stream_tx).await {
                error!("Explore stream error: {e}");
            }
        });

        let mut text_content = String::new();
        let mut thinking_content = String::new();
        let mut pending_tools = Vec::new();
        let mut stop_reason = StopReason::EndTurn;

        loop {
            tokio::select! {
                event = stream_rx.recv() => {
                    let Some(event) = event else { break };
                    match &event {
                        StreamEvent::TextDelta { text } => text_content.push_str(text),
                        StreamEvent::ThinkingDelta { text } => thinking_content.push_str(text),
                        StreamEvent::ToolUseEnd { id, name, input } => {
                            pending_tools.push((id.clone(), name.clone(), input.clone()));
                            tool_call_count += 1;

                            // Track active tool for progress display
                            let arg = get_display_arg(name, input);
                            let truncated = if arg.len() > 60 { arg[..60].to_string() + "…" } else { arg };
                            active_tools.insert(id.clone(), (name.clone(), truncated));

                            // Emit progress showing new tool starting
                            if let Some(ref tx) = progress_tx {
                                let activity = build_explore_activity(
                                    &completed_lines,
                                    &active_tools,
                                    tool_call_count,
                                    total_input + total_output,
                                    started_at.elapsed().as_secs(),
                                );
                                let _ = tx.send(StreamEvent::ExploreProgress {
                                    id: explore_id.clone(),
                                    task: task_display.clone(),
                                    tool_calls: tool_call_count,
                                    tokens: total_input + total_output,
                                    elapsed_secs: started_at.elapsed().as_secs(),
                                    activity,
                                }).await;
                            }
                        }
                        StreamEvent::Usage { input_tokens, output_tokens, .. } => {
                            total_input += input_tokens;
                            total_output += output_tokens;
                        }
                        StreamEvent::MessageEnd { stop_reason: r } => {
                            stop_reason = r.clone();
                        }
                        StreamEvent::Error { error } => {
                            text_output.push_str(&format!("\n[Explore error: {error}]\n"));
                            break 'outer;
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    stream_handle.abort();
                    text_output.push_str("\n[Timeout]");
                    break 'outer;
                }
            }
        }

        let _ = stream_handle.await;

        // Accumulate text output
        if !text_content.is_empty() {
            text_output.push_str(&text_content);
        }

        // Build assistant blocks
        let mut blocks = Vec::new();
        if !thinking_content.is_empty() {
            blocks.push(lukan_core::models::messages::ContentBlock::Thinking {
                text: thinking_content,
            });
        }
        if !text_content.is_empty() {
            blocks.push(lukan_core::models::messages::ContentBlock::Text { text: text_content });
        }
        for (tool_id, name, input) in &pending_tools {
            blocks.push(lukan_core::models::messages::ContentBlock::ToolUse {
                id: tool_id.clone(),
                name: name.clone(),
                input: input.clone(),
            });
        }
        if !blocks.is_empty() {
            history.add_assistant_blocks(blocks);
        }

        turns += 1;

        if stop_reason != StopReason::ToolUse || pending_tools.is_empty() {
            break;
        }

        // Check deadline before tool execution
        if tokio::time::Instant::now() >= deadline {
            text_output.push_str("\n[Timeout]");
            break;
        }

        // Execute tools in parallel, but respect the deadline
        let tool_futures = {
            let mut futs = Vec::new();
            for (_tool_id, name, input) in &pending_tools {
                let reg = Arc::clone(&tools);
                let rf = Arc::clone(&read_files);
                let c = cwd.clone();
                let n = name.clone();
                let inp = input.clone();
                let sandbox_cfg = sandbox.clone();
                let ap = allowed_paths.clone();
                futs.push(tokio::spawn(async move {
                    let ctx = ToolContext {
                        progress_tx: None,
                        event_tx: None,
                        tool_call_id: None,
                        read_files: rf,
                        cwd: c,
                        bg_signal: None,
                        sandbox: sandbox_cfg,
                        allowed_paths: ap,
                        cancel: None,
                    };
                    match reg.execute(&n, inp, &ctx).await {
                        Ok(r) => r,
                        Err(e) => ToolResult::error(format!("Tool error: {e}")),
                    }
                }));
            }
            futs
        };

        // Wait for tools with deadline
        let tool_results = tokio::select! {
            results = async {
                let mut out = Vec::new();
                for handle in tool_futures {
                    out.push(match handle.await {
                        Ok(r) => r,
                        Err(e) => ToolResult::error(format!("Join error: {e}")),
                    });
                }
                out
            } => results,
            _ = tokio::time::sleep_until(deadline) => {
                text_output.push_str("\n[Timeout during tool execution]");
                break 'outer;
            }
        };

        let mut result_blocks = Vec::new();
        for (i, result) in tool_results.into_iter().enumerate() {
            let (tool_id, tool_name, _) = &pending_tools[i];

            // Remove from active, add to completed
            let active_entry = active_tools.remove(tool_id);
            let summary = summarize_result(tool_name, &result.content, result.is_error);
            let display_arg = active_entry
                .map(|(_, arg)| arg)
                .unwrap_or_else(|| "?".to_string());
            completed_lines.push(format!("{tool_name}({display_arg}) → {summary}"));

            // Capture tool results as fallback when model produces no text
            if !result.is_error && !result.content.is_empty() {
                tool_results_fallback.push(format!("[{tool_name}]\n{}", result.content));
            }

            if let Some(ref tx) = progress_tx {
                let activity = build_explore_activity(
                    &completed_lines,
                    &active_tools,
                    tool_call_count,
                    total_input + total_output,
                    started_at.elapsed().as_secs(),
                );
                let _ = tx
                    .send(StreamEvent::ExploreProgress {
                        id: explore_id.clone(),
                        task: task_display.clone(),
                        tool_calls: tool_call_count,
                        tokens: total_input + total_output,
                        elapsed_secs: started_at.elapsed().as_secs(),
                        activity,
                    })
                    .await;
            }

            result_blocks.push(lukan_core::models::messages::ContentBlock::ToolResult {
                tool_use_id: tool_id.clone(),
                content: result.content,
                is_error: if result.is_error { Some(true) } else { None },
                diff: result.diff,
                image: result.image,
            });
        }

        history.add(lukan_core::models::messages::Message {
            role: lukan_core::models::messages::Role::User,
            content: lukan_core::models::messages::MessageContent::Blocks(result_blocks),
            tool_call_id: None,
            name: None,
        });
    }

    // Build final output: prefer text, fall back to tool results
    // On timeout, include partial findings from completed tools
    let mut final_output = text_output.trim().to_string();

    if final_output.is_empty()
        || final_output == "[Timeout]"
        || final_output == "[Timeout during tool execution]"
    {
        // Include raw tool results when there's no text summary
        let fallback = tool_results_fallback.join("\n\n");
        if !fallback.trim().is_empty() {
            if !final_output.is_empty() {
                final_output.push_str("\n\n--- Partial findings before timeout ---\n\n");
            }
            final_output.push_str(fallback.trim());
        }
    }

    if final_output.is_empty() {
        Ok("Explore completed but produced no output.".to_string())
    } else {
        Ok(final_output)
    }
}

// ── Explore Tool ──────────────────────────────────────────────────────────

pub struct ExploreTool;

#[async_trait]
impl Tool for ExploreTool {
    fn name(&self) -> &str {
        "Explore"
    }

    fn description(&self) -> &str {
        "Launch a research sub-agent to explore the codebase. The agent uses read-only tools \
         (ReadFile, Grep, Glob) to investigate and returns detailed findings with file paths \
         and code snippets. Use for broad searches or multi-step investigations. \
         IMPORTANT: Always write the task description in English, regardless of the user's language. \
         Do NOT call this tool multiple times with the same or overlapping tasks — one call per topic. \
         If you need multiple explorations, make each task distinct and specific."
    }

    fn input_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "What to investigate in the codebase (always in English)"
                },
                "maxTurns": {
                    "type": "integer",
                    "description": "Maximum LLM turns before stopping (default: 15)",
                    "default": 15
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 300000)",
                    "default": 300000
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext,
    ) -> anyhow::Result<ToolResult> {
        let task = input
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: task"))?
            .to_string();

        let timeout = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(300_000);

        let max_turns = input.get("maxTurns").and_then(|v| v.as_u64()).unwrap_or(15) as usize;

        // Use the tool_call_id so TUI progress matches the tool_call message
        let explore_id = ctx
            .tool_call_id
            .clone()
            .unwrap_or_else(|| format!("explore-{}", rand::random::<u32>()));

        let progress_tx = ctx.event_tx.clone();

        match run_explore(&task, timeout, max_turns, progress_tx, explore_id).await {
            Ok(output) => Ok(ToolResult::success(output)),
            Err(e) => Ok(ToolResult::error(format!("Explore error: {e}"))),
        }
    }
}

fn format_sub_agent_result(entry: &SubAgentEntry) -> ToolResult {
    let elapsed = entry
        .completed_at
        .map(|c| {
            let dur = c.signed_duration_since(entry.started_at);
            format!("{}s", dur.num_seconds())
        })
        .unwrap_or_else(|| {
            let dur = Utc::now().signed_duration_since(entry.started_at);
            format!("{}s (running)", dur.num_seconds())
        });

    let mut output = entry.text_output.clone();
    if output.len() > 50_000 {
        let half = 25_000;
        output = format!(
            "{}\n\n... (output truncated) ...\n\n{}",
            &output[..half],
            &output[output.len() - half..]
        );
    }

    let header = format!(
        "Status: {}\nTurns: {}/{}\nElapsed: {}\nTask: {}",
        entry.status, entry.turns, entry.max_turns, elapsed, entry.task
    );

    let content = if output.trim().is_empty() {
        format!("{header}\n\n(No text output yet)")
    } else {
        format!("{header}\n\n--- Output ---\n{}", output.trim())
    };

    if entry.status == SubAgentStatus::Error {
        ToolResult::error(content)
    } else {
        ToolResult::success(content)
    }
}
