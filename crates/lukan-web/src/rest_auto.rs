use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use lukan_core::config::ResolvedConfig;
use lukan_core::models::events::StreamEvent;
use lukan_core::models::messages::{Message, MessageContent, Role};
use lukan_providers::{Provider, StreamParams, SystemPrompt, create_provider};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc};

use crate::state::AppState;

// ── Job store ─────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize)]
pub struct AutoJob {
    pub id: String,
    /// "running" | "done" | "failed"
    pub status: String,
    /// Accumulated progress log (one entry per line)
    pub output: String,
    pub summary: Option<String>,
    pub error: Option<String>,
}

static JOBS: OnceLock<Arc<Mutex<HashMap<String, AutoJob>>>> = OnceLock::new();

fn jobs() -> &'static Arc<Mutex<HashMap<String, AutoJob>>> {
    JOBS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

// ── Request types ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AutoRunRequest {
    pub prompt: String,
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
}

fn default_max_turns() -> usize {
    20
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// POST /api/auto/run
/// Starts an autonomous agent run in the background and returns a job_id immediately.
/// The relay has a 60 s timeout so the response must come back fast — the actual
/// work happens in a spawned task and the caller polls GET /api/auto/jobs/:id.
pub async fn start_auto_run(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AutoRunRequest>,
) -> impl IntoResponse {
    let job_id = uuid::Uuid::new_v4().to_string();

    {
        let mut store = jobs().lock().await;
        store.insert(
            job_id.clone(),
            AutoJob {
                id: job_id.clone(),
                status: "running".to_string(),
                output: String::new(),
                summary: None,
                error: None,
            },
        );
    }

    let resolved = state.config.lock().await.clone();
    let id = job_id.clone();
    let prompt = body.prompt.clone();
    let max_turns = body.max_turns;

    tokio::spawn(async move {
        match run_auto_job(&id, &prompt, max_turns, resolved).await {
            Ok(()) => {}
            Err(e) => {
                let mut store = jobs().lock().await;
                if let Some(job) = store.get_mut(&id) {
                    job.status = "failed".to_string();
                    job.error = Some(e.to_string());
                }
            }
        }
    });

    Json(serde_json::json!({ "job_id": job_id, "status": "started" }))
}

/// GET /api/auto/jobs/:id
/// Returns current status and accumulated output for a running or finished job.
pub async fn get_auto_job(Path(id): Path<String>) -> impl IntoResponse {
    let store = jobs().lock().await;
    match store.get(&id) {
        Some(job) => Json(serde_json::to_value(job).unwrap()).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Job not found" })),
        )
            .into_response(),
    }
}

// ── Core auto logic ───────────────────────────────────────────────────────────

const TURN_TIMEOUT_SECS: u64 = 600;

const SUPERVISOR_SYSTEM: &str = "\
You are a senior software engineer supervising an AI coding agent. \
The user gives you a goal and you direct the agent to accomplish it.

Your role:
1. FIRST TURN: Analyze the goal. Decide your strategy — should the agent explore the codebase first? \
   Ask clarifying questions? Start building directly? Write a detailed first instruction for the agent.
2. SUBSEQUENT TURNS: Review what the agent did (tools executed + its response). Then either:
   - Give the next instruction if more work is needed
   - Ask the agent to verify/test if it claims to be done
   - Answer any questions the agent asked
   - Correct course if the agent went in the wrong direction

Response format — respond with exactly ONE of:
INSTRUCT: <detailed instruction for the agent>
VERIFY: <ask the agent to run tests, build, or verify its work>
DONE: <brief summary of what was accomplished>
FAILED: <reason why the goal cannot be achieved>

Guidelines:
- Be specific in instructions. Don't say \"implement auth\" — say \"create src/auth.rs with a JWT middleware using the jsonwebtoken crate\"
- Always VERIFY before declaring DONE — ask the agent to build, test, or demonstrate the feature works
- If the agent asks a question, answer it based on the codebase context and best practices
- If the agent is stuck or looping, try a different approach
- You can ask the agent to explore the codebase first if you need more context";

async fn append(job_id: &str, line: &str) {
    let mut store = jobs().lock().await;
    if let Some(job) = store.get_mut(job_id) {
        job.output.push_str(line);
        job.output.push('\n');
    }
}

async fn run_auto_job(
    job_id: &str,
    goal: &str,
    max_turns: usize,
    resolved: ResolvedConfig,
) -> Result<()> {
    let unlimited = max_turns == 0;

    append(job_id, &format!("Goal: {goal}")).await;
    append(
        job_id,
        &format!(
            "Max turns: {}",
            if unlimited {
                "unlimited".into()
            } else {
                max_turns.to_string()
            }
        ),
    )
    .await;

    let provider: Arc<dyn Provider> = Arc::from(
        create_provider(&resolved).context("No provider configured. Run `lukan setup` first.")?,
    );
    let supervisor: Arc<dyn Provider> =
        Arc::from(create_provider(&resolved).context("Failed to create supervisor provider")?);

    let cwd = std::env::current_dir()?;
    let system_prompt = build_auto_system_prompt().await;
    let tools = lukan_tools::create_default_registry();

    let agent_config = lukan_agent::AgentConfig {
        provider,
        system_prompt,
        cwd: cwd.clone(),
        tools,
        provider_name: format!("{}", resolved.config.provider),
        model_name: resolved.effective_model().unwrap_or_default().to_string(),
        permission_mode: lukan_core::config::PermissionMode::Skip,
        permissions: Default::default(),
        bg_signal: None,
        allowed_paths: Some(vec![cwd.clone()]),
        permission_mode_rx: None,
        approval_rx: None,
        plan_review_rx: None,
        planner_answer_rx: None,
        browser_tools: false,
        skip_session_save: true,
        vision_provider: None,
        extra_env: resolved.credentials.flatten_skill_env(),
        compaction_threshold: None,
    };

    let mut agent = lukan_agent::AgentLoop::new(agent_config).await?;
    agent.set_disabled_tools(
        ["PlannerQuestion", "SubmitPlan"]
            .into_iter()
            .map(String::from)
            .collect(),
    );

    let mut supervisor_history: Vec<Message> = Vec::new();

    append(job_id, "Supervisor analyzing goal...").await;
    let first_instruction = supervisor_think(
        &supervisor,
        &mut supervisor_history,
        &format!(
            "The user wants to achieve this goal:\n\n{goal}\n\n\
             Working directory: {}\n\n\
             Analyze the goal and provide your first INSTRUCT to the agent.",
            cwd.display()
        ),
    )
    .await?;

    let mut current_message = match &first_instruction {
        SupervisorAction::Instruct(msg) | SupervisorAction::Verify(msg) => {
            append(job_id, &format!("Supervisor: {}", first_line(msg))).await;
            format!(
                "You are running in autonomous mode directed by a supervisor.\n\
                 Do NOT use PlannerQuestion or SubmitPlan tools. Work directly.\n\
                 If you have questions, ask them in your response text.\n\
                 When done with the current instruction, summarize what you did.\n\n\
                 ## Instruction\n\n{}",
                msg
            )
        }
        SupervisorAction::Done(summary) => {
            append(job_id, &format!("Done: {summary}")).await;
            let mut store = jobs().lock().await;
            if let Some(job) = store.get_mut(job_id) {
                job.status = "done".to_string();
                job.summary = Some(summary.clone());
            }
            return Ok(());
        }
        SupervisorAction::Failed(reason) => {
            append(job_id, &format!("Failed: {reason}")).await;
            let mut store = jobs().lock().await;
            if let Some(job) = store.get_mut(job_id) {
                job.status = "failed".to_string();
                job.error = Some(reason.clone());
            }
            return Ok(());
        }
    };

    let mut turn = 0;
    loop {
        turn += 1;
        if !unlimited && turn > max_turns {
            append(job_id, &format!("Max turns ({max_turns}) reached.")).await;
            let mut store = jobs().lock().await;
            if let Some(job) = store.get_mut(job_id) {
                job.status = "done".to_string();
                job.summary = Some(format!("Stopped after {max_turns} turns."));
            }
            break;
        }

        let turn_label = if unlimited {
            format!("Turn {turn}")
        } else {
            format!("Turn {turn}/{max_turns}")
        };
        append(job_id, &format!("── {turn_label} ──")).await;

        let result = run_agent_turn(job_id, &mut agent, &current_message).await?;

        append(job_id, "Supervisor reviewing...").await;
        let action = supervisor_think(
            &supervisor,
            &mut supervisor_history,
            &format!(
                "## Agent executed (turn {turn}):\n\
                 ### Tools used:\n{}\n\n\
                 ### Agent response:\n{}\n\n\
                 ### Errors: {}\n\n\
                 What should the agent do next?",
                truncate(&result.tool_summary, 1500),
                truncate(&result.text, 2000),
                result.had_error,
            ),
        )
        .await?;

        match action {
            SupervisorAction::Instruct(msg) | SupervisorAction::Verify(msg) => {
                append(job_id, &format!("Supervisor: {}", first_line(&msg))).await;
                current_message = msg;
            }
            SupervisorAction::Done(summary) => {
                append(job_id, &format!("Done: {summary}")).await;
                let mut store = jobs().lock().await;
                if let Some(job) = store.get_mut(job_id) {
                    job.status = "done".to_string();
                    job.summary = Some(summary);
                }
                break;
            }
            SupervisorAction::Failed(reason) => {
                append(job_id, &format!("Failed: {reason}")).await;
                let mut store = jobs().lock().await;
                if let Some(job) = store.get_mut(job_id) {
                    job.status = "failed".to_string();
                    job.error = Some(reason);
                }
                break;
            }
        }
    }

    Ok(())
}

// ── Supervisor ────────────────────────────────────────────────────────────────

enum SupervisorAction {
    Instruct(String),
    Verify(String),
    Done(String),
    Failed(String),
}

async fn supervisor_think(
    provider: &Arc<dyn Provider>,
    history: &mut Vec<Message>,
    message: &str,
) -> Result<SupervisorAction> {
    history.push(Message {
        role: Role::User,
        content: MessageContent::Text(message.to_string()),
        tool_call_id: None,
        name: None,
    });

    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(64);
    let params = StreamParams {
        system_prompt: SystemPrompt::Text(SUPERVISOR_SYSTEM.to_string()),
        messages: history.clone(),
        tools: vec![],
    };

    let p = provider.clone();
    let handle = tokio::spawn(async move { p.stream(params, event_tx).await });

    let mut response = String::new();
    while let Some(event) = event_rx.recv().await {
        if let StreamEvent::TextDelta { text } = event {
            response.push_str(&text);
        }
    }
    let _ = handle.await?;

    let response = response.trim().to_string();

    history.push(Message {
        role: Role::Assistant,
        content: MessageContent::Text(response.clone()),
        tool_call_id: None,
        name: None,
    });

    Ok(parse_supervisor_response(&response))
}

fn parse_supervisor_response(response: &str) -> SupervisorAction {
    let trimmed = response.trim();
    if let Some(msg) = trimmed.strip_prefix("INSTRUCT:") {
        SupervisorAction::Instruct(msg.trim().to_string())
    } else if let Some(msg) = trimmed.strip_prefix("VERIFY:") {
        SupervisorAction::Verify(msg.trim().to_string())
    } else if let Some(msg) = trimmed.strip_prefix("DONE:") {
        SupervisorAction::Done(msg.trim().to_string())
    } else if trimmed.starts_with("DONE") {
        SupervisorAction::Done("Goal completed.".to_string())
    } else if let Some(msg) = trimmed.strip_prefix("FAILED:") {
        SupervisorAction::Failed(msg.trim().to_string())
    } else {
        SupervisorAction::Instruct(trimmed.to_string())
    }
}

// ── Agent turn ────────────────────────────────────────────────────────────────

struct TurnResult {
    text: String,
    tool_summary: String,
    had_error: bool,
}

async fn run_agent_turn(
    job_id: &str,
    agent: &mut lukan_agent::AgentLoop,
    message: &str,
) -> Result<TurnResult> {
    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);

    let jid = job_id.to_string();
    let collector = tokio::spawn(async move {
        let mut text = String::new();
        let mut had_error = false;
        let mut tools_used: Vec<String> = Vec::new();

        while let Some(event) = event_rx.recv().await {
            match &event {
                StreamEvent::TextDelta { text: t } => {
                    text.push_str(t);
                }
                StreamEvent::ToolUseStart { name, .. } => {
                    append(&jid, &format!("  ● {name}")).await;
                }
                StreamEvent::ToolResult {
                    name,
                    content,
                    is_error,
                    ..
                } => {
                    let failed = is_error == &Some(true);
                    if failed {
                        had_error = true;
                    }
                    let icon = if failed { "✗" } else { "✓" };
                    let preview = truncate_str(content, 200);
                    append(&jid, &format!("    {icon} {name}: {preview}")).await;
                    tools_used.push(format!("{} {} ({})", icon, name, truncate_str(content, 80)));
                }
                StreamEvent::Error { error } => {
                    append(&jid, &format!("  ✗ Error: {error}")).await;
                    had_error = true;
                }
                _ => {}
            }
        }

        let tool_summary = if tools_used.is_empty() {
            "No tools used.".to_string()
        } else {
            tools_used.join("\n")
        };
        TurnResult {
            text,
            tool_summary,
            had_error,
        }
    });

    match tokio::time::timeout(
        Duration::from_secs(TURN_TIMEOUT_SECS),
        agent.run_turn(message, event_tx, None, None),
    )
    .await
    {
        Ok(result) => result?,
        Err(_) => {
            append(
                job_id,
                &format!("⚠ Turn timed out after {TURN_TIMEOUT_SECS}s"),
            )
            .await;
        }
    }

    collector.await.context("Event collector panicked")
}

// ── System prompt ─────────────────────────────────────────────────────────────

async fn build_auto_system_prompt() -> SystemPrompt {
    use lukan_core::config::LukanPaths;

    const BASE: &str = include_str!("../../../prompts/base.txt");

    let mut cached = vec![BASE.to_string()];

    let global_path = LukanPaths::global_memory_file();
    if let Ok(memory) = tokio::fs::read_to_string(&global_path).await {
        let trimmed = memory.trim();
        if !trimmed.is_empty() {
            cached.push(format!("## Global Memory\n\n{trimmed}"));
        }
    }

    let active_path = LukanPaths::project_memory_active_file();
    if tokio::fs::metadata(&active_path).await.is_ok() {
        let project_path = LukanPaths::project_memory_file();
        if let Ok(memory) = tokio::fs::read_to_string(&project_path).await {
            let trimmed = memory.trim();
            if !trimmed.is_empty() {
                cached.push(format!("## Project Memory\n\n{trimmed}"));
            }
        }
    }

    let plugins_dir = LukanPaths::plugins_dir();
    if let Ok(mut entries) = tokio::fs::read_dir(&plugins_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let prompt_path = entry.path().join("prompt.txt");
            if let Ok(prompt) = tokio::fs::read_to_string(&prompt_path).await {
                let trimmed = prompt.trim();
                if !trimmed.is_empty() {
                    cached.push(trimmed.to_string());
                }
            }
        }
    }

    let now = chrono::Utc::now();
    let dynamic = format!("Current date/time: {} UTC", now.format("%Y-%m-%d %H:%M"));

    SystemPrompt::Structured { cached, dynamic }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}…\n[truncated]", &s[..max])
    } else {
        s.to_string()
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}…", &s[..max])
    } else {
        s.to_string()
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("(instruction)")
}
