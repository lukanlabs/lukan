use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use lukan_core::config::{CredentialsManager, ConfigManager, ResolvedConfig};
use lukan_core::models::events::StreamEvent;
use lukan_providers::{Provider, StreamParams, SystemPrompt, create_provider};

const MAX_TURNS: usize = 50;
const TURN_TIMEOUT_SECS: u64 = 600; // 10 minutes per turn

/// Run autonomous agent: takes a goal, runs an agent in skip mode, and uses
/// a supervisor LLM to evaluate progress and send follow-up messages.
pub async fn run_auto(goal: &str, max_turns: Option<usize>) -> Result<()> {
    let max_turns = max_turns.unwrap_or(MAX_TURNS);

    eprintln!("\x1b[1;36m▶ Autonomous mode\x1b[0m");
    eprintln!("  Goal: {goal}");
    eprintln!("  Max turns: {max_turns}");
    eprintln!();

    // Load config and create provider
    let config = ConfigManager::load().await?;
    let credentials = CredentialsManager::load().await?;
    let resolved = ResolvedConfig { config, credentials };

    let provider: Arc<dyn Provider> = Arc::from(
        create_provider(&resolved).context("No provider configured. Run `lukan setup` first.")?,
    );

    // Create a second provider for the supervisor evaluations
    let supervisor: Arc<dyn Provider> = Arc::from(
        create_provider(&resolved).context("Failed to create supervisor provider")?,
    );

    let cwd = std::env::current_dir()?;

    // Build system prompt (same as TUI/web)
    let system_prompt = build_auto_system_prompt().await;

    // Create tools registry
    let tools = lukan_tools::create_default_registry();

    // Create agent in skip mode
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
    let session_id = agent.session_id().to_string();
    eprintln!("  Session: {session_id}");
    eprintln!();

    // Build initial prompt
    let initial_prompt = format!(
        "You are running in autonomous mode. Complete the following goal without asking questions.\n\
         If you need to make decisions, use your best judgment.\n\
         When you are completely done, say: GOAL_COMPLETE\n\
         If the goal is impossible, say: GOAL_FAILED followed by the reason.\n\n\
         ## Goal\n\n{goal}"
    );

    let mut turn = 0;
    let mut current_message = initial_prompt;

    loop {
        turn += 1;
        if turn > max_turns {
            eprintln!("\n\x1b[1;33m⚠ Max turns ({max_turns}) reached.\x1b[0m");
            break;
        }

        eprintln!("\x1b[1;34m── Turn {turn}/{max_turns} ──\x1b[0m");

        // Run agent turn and collect events
        let (text_response, had_error) = run_agent_turn(&mut agent, &current_message).await?;

        // Check for explicit completion signals
        if text_response.contains("GOAL_COMPLETE") {
            eprintln!("\n\x1b[1;32m✓ Goal completed in {turn} turns.\x1b[0m");
            break;
        }
        if text_response.contains("GOAL_FAILED") {
            eprintln!("\n\x1b[1;31m✗ Agent reported failure.\x1b[0m");
            break;
        }

        // Supervisor evaluates progress
        match evaluate_progress(&supervisor, goal, &text_response, turn, had_error).await? {
            Decision::Done => {
                eprintln!("\n\x1b[1;32m✓ Goal achieved in {turn} turns.\x1b[0m");
                break;
            }
            Decision::Continue { message } => {
                eprintln!(
                    "\x1b[36m  ▸ {}\x1b[0m",
                    message.lines().next().unwrap_or("(follow-up)")
                );
                current_message = message;
            }
            Decision::Failed { reason } => {
                eprintln!("\n\x1b[1;31m✗ {reason}\x1b[0m");
                break;
            }
        }
    }

    eprintln!("\n  Session: {session_id}");
    eprintln!("  Resume: lukan -c");

    Ok(())
}

/// Run a single agent turn, printing events to stderr, returning the text response.
async fn run_agent_turn(
    agent: &mut lukan_agent::AgentLoop,
    message: &str,
) -> Result<(String, bool)> {
    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);

    // Spawn event consumer
    let collector = tokio::spawn(async move {
        let mut text = String::new();
        let mut had_error = false;

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
                    let icon = if is_error == &Some(true) {
                        had_error = true;
                        "\x1b[31m✗\x1b[0m"
                    } else {
                        "\x1b[32m✓\x1b[0m"
                    };
                    let preview = if content.len() > 120 {
                        format!("{}…", &content[..120])
                    } else {
                        content.clone()
                    };
                    eprintln!("    {icon} {name}: {preview}");
                }
                StreamEvent::Error { error } => {
                    eprintln!("\x1b[31m  ✗ {error}\x1b[0m");
                    had_error = true;
                }
                _ => {}
            }
        }
        eprintln!();
        (text, had_error)
    });

    // Run the turn with a timeout
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

    Ok(collector.await?)
}

enum Decision {
    Done,
    Continue { message: String },
    Failed { reason: String },
}

/// Supervisor LLM evaluates whether the goal is complete.
async fn evaluate_progress(
    provider: &Arc<dyn Provider>,
    goal: &str,
    last_response: &str,
    turn: usize,
    had_error: bool,
) -> Result<Decision> {
    // Truncate response if too long
    let response_preview = if last_response.len() > 2000 {
        format!("{}…\n[truncated]", &last_response[..2000])
    } else {
        last_response.to_string()
    };

    let prompt = format!(
        "## Original Goal\n{goal}\n\n\
         ## Agent Response (turn {turn})\n{response_preview}\n\n\
         ## Status\nErrors: {had_error}\n\n\
         Respond with exactly one of:\n\
         DONE — if the goal is fully complete\n\
         CONTINUE: <next instruction> — if more work is needed\n\
         FAILED: <reason> — if stuck or impossible"
    );

    let messages = vec![lukan_core::models::messages::Message {
        role: lukan_core::models::messages::Role::User,
        content: lukan_core::models::messages::MessageContent::Text(prompt),
        tool_call_id: None,
        name: None,
    }];

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(64);
    let p = provider.clone();
    let handle = tokio::spawn(async move {
        p.stream(
            StreamParams {
                system_prompt: SystemPrompt::Text(
                    "You are a concise supervisor. Respond with DONE, CONTINUE: <message>, or FAILED: <reason>. Nothing else.".to_string(),
                ),
                messages,
                tools: vec![],
            },
            tx,
        )
        .await
    });

    let mut response = String::new();
    while let Some(event) = rx.recv().await {
        if let StreamEvent::TextDelta { text } = event {
            response.push_str(&text);
        }
    }
    let _ = handle.await?;

    let response = response.trim();
    if response.starts_with("DONE") {
        Ok(Decision::Done)
    } else if let Some(msg) = response.strip_prefix("CONTINUE:") {
        Ok(Decision::Continue {
            message: msg.trim().to_string(),
        })
    } else if let Some(reason) = response.strip_prefix("FAILED:") {
        Ok(Decision::Failed {
            reason: reason.trim().to_string(),
        })
    } else {
        // Default: treat unknown response as continue
        Ok(Decision::Continue {
            message: response.to_string(),
        })
    }
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
