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
) {
    let mut mgr = MANAGER.write().await;
    mgr.provider = Some(provider);
    mgr.system_prompt = Some(system_prompt);
    mgr.cwd = Some(cwd);
    mgr.provider_name = Some(provider_name);
    mgr.model_name = Some(model_name);
}

// ── Manager ───────────────────────────────────────────────────────────────

struct SubAgentManager {
    entries: HashMap<String, SubAgentEntry>,
    provider: Option<Arc<dyn Provider>>,
    system_prompt: Option<SystemPrompt>,
    cwd: Option<std::path::PathBuf>,
    provider_name: Option<String>,
    model_name: Option<String>,
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
        }
    }
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

    let (provider, system_prompt, cwd, _provider_name, _model_name) = {
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
        (provider, sp, cwd, pn, mn)
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
        )
        .await;
    });

    Ok(id)
}

async fn run_sub_agent(
    id: String,
    task: String,
    timeout_ms: u64,
    max_turns: usize,
    provider: Arc<dyn Provider>,
    system_prompt: SystemPrompt,
    cwd: std::path::PathBuf,
) {
    let mut history = MessageHistory::new();
    history.add_user_message(&task);

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
                entry.input_tokens = total_input;
                entry.output_tokens = total_output;
            }
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

            handles.push(tokio::spawn(async move {
                let ctx = ToolContext {
                    progress_tx: None,
                    read_files: rf,
                    cwd: c,
                    bg_signal: None,
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
            let (tool_id, _, _) = &pending_tools[i];
            result_blocks.push(lukan_core::models::messages::ContentBlock::ToolResult {
                tool_use_id: tool_id.clone(),
                content: result.content.clone(),
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

    // Finalize
    {
        let mut mgr = MANAGER.write().await;
        if let Some(entry) = mgr.entries.get_mut(&id) {
            entry.status = final_status;
            entry.completed_at = Some(Utc::now());
            entry.turns = turns;
            entry.text_output = text_output;
            entry.input_tokens = total_input;
            entry.output_tokens = total_output;
            entry.error = final_error;
        }
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
