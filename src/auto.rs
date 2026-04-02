use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use lukan_core::config::{ConfigManager, CredentialsManager, ResolvedConfig};
use lukan_core::models::events::StreamEvent;
use lukan_core::models::messages::{Message, MessageContent, Role};
use lukan_providers::{Provider, StreamParams, SystemPrompt, create_provider};

const TURN_TIMEOUT_SECS: u64 = 600; // 10 minutes per turn

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

/// Run autonomous agent directed by a supervisor LLM.
/// The supervisor processes the user's goal, plans the approach, and sends
/// instructions to the agent. The agent executes and reports back.
pub async fn run_auto(goal: &str, max_turns: usize) -> Result<()> {
    let unlimited = max_turns == 0;

    eprintln!("\x1b[1;36m▶ Autonomous mode \x1b[33m[BETA]\x1b[0m");
    eprintln!("  Goal: {goal}");
    eprintln!(
        "  Max turns: {}",
        if unlimited {
            "unlimited".to_string()
        } else {
            max_turns.to_string()
        }
    );
    eprintln!();

    // Load config and create providers
    let config = ConfigManager::load().await?;
    let credentials = CredentialsManager::load().await?;
    let resolved = ResolvedConfig {
        config,
        credentials,
    };

    let provider: Arc<dyn Provider> = Arc::from(
        create_provider(&resolved).context("No provider configured. Run `lukan setup` first.")?,
    );
    let supervisor: Arc<dyn Provider> =
        Arc::from(create_provider(&resolved).context("Failed to create supervisor provider")?);

    let cwd = std::env::current_dir()?;

    // Build agent
    let system_prompt = build_auto_system_prompt().await;
    let tools = lukan_tools::create_default_registry();

    let agent_config = lukan_agent::AgentConfig {
        provider: provider.clone(),
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
        skip_session_save: false,
        vision_provider: None,
        extra_env: resolved.credentials.flatten_skill_env(),
    };

    let mut agent = lukan_agent::AgentLoop::new(agent_config).await?;
    agent.set_disabled_tools(
        ["PlannerQuestion", "SubmitPlan"]
            .into_iter()
            .map(String::from)
            .collect(),
    );

    let session_id = agent.session_id().to_string();
    eprintln!("  Session: {session_id}");
    eprintln!();

    // Supervisor conversation history (persists across turns for context)
    let mut supervisor_history: Vec<Message> = Vec::new();

    // Phase 1: Supervisor processes the goal and creates first instruction
    eprintln!("\x1b[1;35m── Supervisor analyzing goal ──\x1b[0m");
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
            eprintln!("\x1b[36m  ▸ {}\x1b[0m", first_line(msg));
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
            eprintln!("\n\x1b[1;32m✓ Supervisor: {summary}\x1b[0m");
            eprintln!("\n  Session: {session_id}");
            return Ok(());
        }
        SupervisorAction::Failed(reason) => {
            eprintln!("\n\x1b[1;31m✗ Supervisor: {reason}\x1b[0m");
            eprintln!("\n  Session: {session_id}");
            return Ok(());
        }
    };

    // Phase 2: Agent execution loop
    let mut turn = 0;
    loop {
        turn += 1;
        if !unlimited && turn > max_turns {
            eprintln!("\n\x1b[1;33m⚠ Max turns ({max_turns}) reached.\x1b[0m");
            break;
        }

        let turn_label = if unlimited {
            format!("Turn {turn}")
        } else {
            format!("Turn {turn}/{max_turns}")
        };
        eprintln!("\x1b[1;34m── {turn_label} ──\x1b[0m");

        // Run agent turn
        let result = run_agent_turn(&mut agent, &current_message).await?;

        // Supervisor reviews the result
        eprintln!("\x1b[1;35m── Supervisor reviewing ──\x1b[0m");
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
                eprintln!("\x1b[36m  ▸ {}\x1b[0m", first_line(&msg));
                current_message = msg;
            }
            SupervisorAction::Done(summary) => {
                eprintln!("\n\x1b[1;32m✓ {summary}\x1b[0m");
                break;
            }
            SupervisorAction::Failed(reason) => {
                eprintln!("\n\x1b[1;31m✗ {reason}\x1b[0m");
                break;
            }
        }
    }

    eprintln!("\n  Session: {session_id}");
    eprintln!("  Resume: lukan -c");
    Ok(())
}

// ── Supervisor ──────────────────────────────────────────────────────────

enum SupervisorAction {
    Instruct(String),
    Verify(String),
    Done(String),
    Failed(String),
}

/// Send a message to the supervisor and get its decision.
/// Maintains conversation history so the supervisor has full context.
async fn supervisor_think(
    provider: &Arc<dyn Provider>,
    history: &mut Vec<Message>,
    message: &str,
) -> Result<SupervisorAction> {
    // Add user message to supervisor history
    history.push(Message {
        role: Role::User,
        content: MessageContent::Text(message.to_string()),
        tool_call_id: None,
        name: None,
    });

    // Call LLM
    let (tx, mut rx) = mpsc::channel::<StreamEvent>(64);
    let params = StreamParams {
        system_prompt: SystemPrompt::Text(SUPERVISOR_SYSTEM.to_string()),
        messages: history.clone(),
        tools: vec![],
    };

    let p = provider.clone();
    let handle = tokio::spawn(async move { p.stream(params, tx).await });

    let mut response = String::new();
    while let Some(event) = rx.recv().await {
        if let StreamEvent::TextDelta { text } = event {
            response.push_str(&text);
        }
    }
    let _ = handle.await?;

    let response = response.trim().to_string();

    // Add assistant response to history
    history.push(Message {
        role: Role::Assistant,
        content: MessageContent::Text(response.clone()),
        tool_call_id: None,
        name: None,
    });

    // Parse decision
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
        // Default: treat as instruction
        SupervisorAction::Instruct(trimmed.to_string())
    }
}

// ── Agent turn ──────────────────────────────────────────────────────────

struct TurnResult {
    text: String,
    tool_summary: String,
    had_error: bool,
}

async fn run_agent_turn(agent: &mut lukan_agent::AgentLoop, message: &str) -> Result<TurnResult> {
    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);

    let collector = tokio::spawn(async move {
        let mut text = String::new();
        let mut had_error = false;
        let mut tools_used: Vec<String> = Vec::new();

        while let Some(event) = event_rx.recv().await {
            match &event {
                StreamEvent::TextDelta { text: t } => {
                    eprint!("{t}");
                    text.push_str(t);
                }
                StreamEvent::ThinkingDelta { text: t } => {
                    eprint!("\x1b[2m{t}\x1b[0m");
                }
                StreamEvent::ToolUseStart { name, .. } => {
                    eprintln!("\n\x1b[33m  ● {name}\x1b[0m");
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
                    let icon = if failed {
                        "\x1b[31m✗\x1b[0m"
                    } else {
                        "\x1b[32m✓\x1b[0m"
                    };
                    let preview = truncate_str(content, 120);
                    eprintln!("    {icon} {name}: {preview}");
                    tools_used.push(format!(
                        "{} {} ({})",
                        if failed { "✗" } else { "✓" },
                        name,
                        truncate_str(content, 80)
                    ));
                }
                StreamEvent::Error { error } => {
                    eprintln!("\x1b[31m  ✗ {error}\x1b[0m");
                    had_error = true;
                }
                _ => {}
            }
        }
        eprintln!();
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
            eprintln!("\x1b[33m  ⚠ Turn timed out after {TURN_TIMEOUT_SECS}s\x1b[0m");
        }
    }

    collector.await.context("Event collector panicked")
}

// ── Helpers ─────────────────────────────────────────────────────────────

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

/// Build system prompt for autonomous mode (reuses base prompt + memory)
async fn build_auto_system_prompt() -> SystemPrompt {
    use lukan_core::config::LukanPaths;

    const BASE: &str = include_str!("../prompts/base.txt");

    let mut cached = vec![BASE.to_string()];

    // Global memory
    let global_path = LukanPaths::global_memory_file();
    if let Ok(memory) = tokio::fs::read_to_string(&global_path).await {
        let trimmed = memory.trim();
        if !trimmed.is_empty() {
            cached.push(format!("## Global Memory\n\n{trimmed}"));
        }
    }

    // Project memory
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

    // Plugin prompts
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
