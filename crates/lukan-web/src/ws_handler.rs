use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    response::IntoResponse,
};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use lukan_agent::{AgentConfig, AgentLoop, SessionManager};
use lukan_core::config::LukanPaths;
use lukan_core::models::events::StreamEvent;
use lukan_providers::{SystemPrompt, create_provider};
use lukan_tools::create_default_registry;

use crate::protocol::{ClientMessage, ServerMessage, TokenUsage};
use crate::state::AppState;

/// WebSocket upgrade handler
pub async fn ws_upgrade_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_connection(socket, state))
}

/// Handle a single WebSocket connection
async fn handle_connection(socket: WebSocket, state: Arc<AppState>) {
    let conn_id = state.next_connection_id();
    info!(conn_id, "WebSocket connected");

    let (mut ws_tx, mut ws_rx) = socket.split();
    use futures::SinkExt;
    use futures::StreamExt;

    let mut authenticated = !state.auth_required();

    // If auth required, send auth_required message
    if !authenticated {
        let msg = ServerMessage::AuthRequired;
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = ws_tx.send(Message::Text(json.into())).await;
        }
    } else {
        // Send init state
        send_init(&state, &mut ws_tx).await;
    }

    // Message loop
    while let Some(Ok(msg)) = ws_rx.next().await {
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Close(_) => break,
            _ => continue,
        };

        let client_msg: ClientMessage = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                warn!(conn_id, error = %e, "Invalid client message");
                send_json(&mut ws_tx, &ServerMessage::Error {
                    error: format!("Invalid message: {e}"),
                })
                .await;
                continue;
            }
        };

        // Handle auth messages before checking authentication
        match &client_msg {
            ClientMessage::Auth { token } => {
                if state.verify_token(token) {
                    authenticated = true;
                    let new_token = crate::auth::create_auth_token(
                        &state.auth_secret,
                        state.token_ttl_ms,
                    );
                    send_json(&mut ws_tx, &ServerMessage::AuthOk { token: new_token }).await;
                    send_init(&state, &mut ws_tx).await;
                } else {
                    send_json(&mut ws_tx, &ServerMessage::AuthError {
                        error: "Invalid or expired token".to_string(),
                    })
                    .await;
                }
                continue;
            }
            ClientMessage::AuthLogin { password } => {
                match state.validate_password(password) {
                    Some(token) => {
                        authenticated = true;
                        send_json(&mut ws_tx, &ServerMessage::AuthOk { token }).await;
                        send_init(&state, &mut ws_tx).await;
                    }
                    None => {
                        send_json(&mut ws_tx, &ServerMessage::AuthError {
                            error: "Invalid password".to_string(),
                        })
                        .await;
                    }
                }
                continue;
            }
            _ => {}
        }

        // Gate all other messages behind authentication
        if !authenticated {
            send_json(&mut ws_tx, &ServerMessage::AuthRequired).await;
            continue;
        }

        // Dispatch authenticated messages
        dispatch_message(client_msg, conn_id, &state, &mut ws_tx).await;
    }

    // On disconnect: release processing lock if owned, save session
    {
        let mut owner = state.processing_owner.lock().await;
        if *owner == Some(conn_id) {
            *owner = None;
            info!(conn_id, "Released processing lock on disconnect");
        }
    }

    // Save session if agent exists
    {
        let mut agent_lock = state.agent.lock().await;
        if let Some(ref mut agent) = *agent_lock
            && let Err(e) = agent.save_session_public().await
        {
            error!(conn_id, error = %e, "Failed to save session on disconnect");
        }
    }

    info!(conn_id, "WebSocket disconnected");
}

/// Dispatch an authenticated client message
async fn dispatch_message(
    msg: ClientMessage,
    conn_id: usize,
    state: &Arc<AppState>,
    ws_tx: &mut futures::stream::SplitSink<WebSocket, Message>,
) {
    match msg {
        ClientMessage::SendMessage { content } => {
            handle_send_message(conn_id, &content, state, ws_tx).await;
        }

        ClientMessage::Abort => {
            // Release processing lock
            let mut owner = state.processing_owner.lock().await;
            if *owner == Some(conn_id) {
                *owner = None;
                info!(conn_id, "Aborted processing");
            }
            // Note: the actual abort mechanism works by dropping the event channel
            // sender, which causes the agent's event_tx.send() to fail.
        }

        ClientMessage::LoadSession { session_id } => {
            handle_load_session(&session_id, state, ws_tx).await;
        }

        ClientMessage::NewSession { name: _ } => {
            handle_new_session(state, ws_tx).await;
        }

        ClientMessage::ListSessions => {
            match SessionManager::list().await {
                Ok(sessions) => {
                    send_json(ws_tx, &ServerMessage::SessionList { sessions }).await;
                }
                Err(e) => {
                    send_json(ws_tx, &ServerMessage::Error {
                        error: format!("Failed to list sessions: {e}"),
                    })
                    .await;
                }
            }
        }

        ClientMessage::DeleteSession { session_id } => {
            match SessionManager::delete(&session_id).await {
                Ok(_) => {
                    // Re-send session list
                    if let Ok(sessions) = SessionManager::list().await {
                        send_json(ws_tx, &ServerMessage::SessionList { sessions }).await;
                    }
                }
                Err(e) => {
                    send_json(ws_tx, &ServerMessage::Error {
                        error: format!("Failed to delete session: {e}"),
                    })
                    .await;
                }
            }
        }

        ClientMessage::ListModels => {
            let config = state.config.lock().await;
            let models = config
                .config
                .models
                .clone()
                .unwrap_or_default();
            let provider_name = state.provider_name.lock().await.clone();
            let model_name = state.model_name.lock().await.clone();
            let current = format!("{provider_name}:{model_name}");
            send_json(ws_tx, &ServerMessage::ModelList { models, current }).await;
        }

        ClientMessage::SetModel { model } => {
            handle_set_model(&model, state, ws_tx).await;
        }

        ClientMessage::GetConfig => {
            let config = state.config.lock().await;
            let safe = serde_json::json!({
                "maxTokens": config.config.max_tokens,
                "temperature": config.config.temperature,
                "timezone": config.config.timezone,
                "syntaxTheme": config.config.syntax_theme,
            });
            send_json(ws_tx, &ServerMessage::ConfigValues { config: safe }).await;
        }

        ClientMessage::SetConfig { config: new_values } => {
            handle_set_config(new_values, state, ws_tx).await;
        }

        ClientMessage::SetPermissionMode { mode } => {
            send_json(ws_tx, &ServerMessage::ModeChanged { mode }).await;
        }

        // Stubs — send empty updates or errors
        ClientMessage::Approve { .. } | ClientMessage::DenyAll => {
            send_json(ws_tx, &ServerMessage::Error {
                error: "Approval flow not yet implemented in Rust backend. Tools are auto-approved.".to_string(),
            })
            .await;
        }

        ClientMessage::AnswerQuestion { .. } => {
            send_json(ws_tx, &ServerMessage::Error {
                error: "Question answering not yet implemented".to_string(),
            })
            .await;
        }

        ClientMessage::SetScreenshots { enabled } => {
            send_json(ws_tx, &ServerMessage::ScreenshotsChanged { enabled: false }).await;
            let _ = enabled;
        }

        ClientMessage::GetSubAgentDetail { .. } | ClientMessage::AbortSubAgent { .. } => {
            send_json(ws_tx, &ServerMessage::SubAgentsUpdate {
                agents: vec![],
            })
            .await;
        }

        ClientMessage::ListWorkers
        | ClientMessage::CreateWorker { .. }
        | ClientMessage::UpdateWorker { .. }
        | ClientMessage::DeleteWorker { .. }
        | ClientMessage::ToggleWorker { .. }
        | ClientMessage::GetWorkerDetail { .. }
        | ClientMessage::GetWorkerRunDetail { .. } => {
            send_json(ws_tx, &ServerMessage::WorkersUpdate {
                workers: vec![],
            })
            .await;
        }

        ClientMessage::PlanAccept { .. }
        | ClientMessage::PlanReject { .. }
        | ClientMessage::PlanTaskFeedback { .. } => {
            send_json(ws_tx, &ServerMessage::Error {
                error: "Plan review not yet implemented".to_string(),
            })
            .await;
        }

        // Auth messages handled above
        ClientMessage::Auth { .. } | ClientMessage::AuthLogin { .. } => {}
    }
}

/// Handle send_message: acquire lock, run agent turn, stream events
async fn handle_send_message(
    conn_id: usize,
    content: &str,
    state: &Arc<AppState>,
    ws_tx: &mut futures::stream::SplitSink<WebSocket, Message>,
) {
    use futures::SinkExt;

    // Try to acquire processing lock
    {
        let mut owner = state.processing_owner.lock().await;
        if owner.is_some() {
            send_json(ws_tx, &ServerMessage::Error {
                error: "Another client is currently processing. Please wait.".to_string(),
            })
            .await;
            return;
        }
        *owner = Some(conn_id);
    }

    // Ensure agent exists
    {
        let mut agent_lock = state.agent.lock().await;
        if agent_lock.is_none() {
            match create_agent(state).await {
                Ok(agent) => {
                    *agent_lock = Some(agent);
                }
                Err(e) => {
                    let mut owner = state.processing_owner.lock().await;
                    *owner = None;
                    send_json(ws_tx, &ServerMessage::Error {
                        error: format!("Failed to create agent: {e}"),
                    })
                    .await;
                    return;
                }
            }
        }
    }

    // Take the agent out of the mutex for the duration of the turn
    let mut agent = {
        let mut lock = state.agent.lock().await;
        lock.take().unwrap()
    };

    // Create channel for streaming events
    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);

    let content_owned = content.to_string();

    // Spawn the agent turn
    let agent_handle = tokio::spawn(async move {
        let result = agent.run_turn(&content_owned, event_tx).await;
        (agent, result)
    });

    // Forward stream events to WebSocket
    while let Some(event) = event_rx.recv().await {
        if let Ok(json) = serde_json::to_string(&event)
            && ws_tx.send(Message::Text(json.into())).await.is_err()
        {
            warn!(conn_id, "WebSocket send failed, client likely disconnected");
            break;
        }
    }

    // Wait for agent turn to complete
    match agent_handle.await {
        Ok((returned_agent, result)) => {
            if let Err(e) = result {
                error!(conn_id, error = %e, "Agent turn error");
                send_json(ws_tx, &ServerMessage::Error {
                    error: format!("Agent error: {e}"),
                })
                .await;
            }

            // Send processing_complete with updated state
            let messages = returned_agent.messages_json();
            let checkpoints = returned_agent.checkpoints().to_vec();
            let context_size = returned_agent.last_context_size();

            send_json(ws_tx, &ServerMessage::ProcessingComplete {
                messages,
                checkpoints,
                context_size: Some(context_size),
            })
            .await;

            // Put agent back
            let mut lock = state.agent.lock().await;
            *lock = Some(returned_agent);
        }
        Err(e) => {
            error!(conn_id, error = %e, "Agent task panicked");
            send_json(ws_tx, &ServerMessage::Error {
                error: format!("Agent task failed: {e}"),
            })
            .await;
        }
    }

    // Release processing lock
    let mut owner = state.processing_owner.lock().await;
    if *owner == Some(conn_id) {
        *owner = None;
    }
}

/// Handle loading an existing session
async fn handle_load_session(
    session_id: &str,
    state: &Arc<AppState>,
    ws_tx: &mut futures::stream::SplitSink<WebSocket, Message>,
) {
    // Save current session first
    {
        let mut agent_lock = state.agent.lock().await;
        if let Some(ref mut agent) = *agent_lock {
            let _ = agent.save_session_public().await;
        }
    }

    // Create new agent with loaded session
    match create_agent_with_session(state, session_id).await {
        Ok(agent) => {
            let messages = agent.messages_json();
            let checkpoints = agent.checkpoints().to_vec();
            let input_tokens = agent.input_tokens();
            let output_tokens = agent.output_tokens();
            let context_size = agent.last_context_size();
            let sid = agent.session_id().to_string();

            let mut agent_lock = state.agent.lock().await;
            *agent_lock = Some(agent);

            send_json(ws_tx, &ServerMessage::SessionLoaded {
                session_id: sid,
                messages,
                checkpoints,
                token_usage: TokenUsage {
                    input: input_tokens,
                    output: output_tokens,
                    cache_creation: None,
                    cache_read: None,
                },
                context_size,
            })
            .await;
        }
        Err(e) => {
            send_json(ws_tx, &ServerMessage::Error {
                error: format!("Failed to load session: {e}"),
            })
            .await;
        }
    }
}

/// Handle creating a new session
async fn handle_new_session(
    state: &Arc<AppState>,
    ws_tx: &mut futures::stream::SplitSink<WebSocket, Message>,
) {
    // Save current session
    {
        let mut agent_lock = state.agent.lock().await;
        if let Some(ref mut agent) = *agent_lock {
            let _ = agent.save_session_public().await;
        }
    }

    // Create new agent
    match create_agent(state).await {
        Ok(agent) => {
            let mut agent_lock = state.agent.lock().await;
            *agent_lock = Some(agent);
        }
        Err(e) => {
            send_json(ws_tx, &ServerMessage::Error {
                error: format!("Failed to create session: {e}"),
            })
            .await;
            return;
        }
    }

    // Send init with new session state
    send_init(state, ws_tx).await;
}

/// Handle model switching
async fn handle_set_model(
    model: &str,
    state: &Arc<AppState>,
    ws_tx: &mut futures::stream::SplitSink<WebSocket, Message>,
) {
    // Parse "provider:model" format
    let (provider_str, model_str) = if let Some(colon) = model.find(':') {
        (model[..colon].to_string(), model[colon + 1..].to_string())
    } else {
        // Default: keep current provider, just change model
        let current = state.provider_name.lock().await.clone();
        (current, model.to_string())
    };

    // Update config and create new provider
    let new_provider = {
        let mut config = state.config.lock().await;
        config.config.provider = match serde_json::from_value(serde_json::Value::String(
            provider_str.to_string(),
        )) {
            Ok(p) => p,
            Err(e) => {
                send_json(ws_tx, &ServerMessage::Error {
                    error: format!("Invalid provider: {e}"),
                })
                .await;
                return;
            }
        };
        config.config.model = Some(model_str.to_string());

        match create_provider(&config) {
            Ok(p) => p,
            Err(e) => {
                send_json(ws_tx, &ServerMessage::Error {
                    error: format!("Failed to create provider: {e}"),
                })
                .await;
                return;
            }
        }
    };

    // Swap provider on agent if it exists
    {
        let mut agent_lock = state.agent.lock().await;
        if let Some(ref mut agent) = *agent_lock {
            agent.swap_provider(Arc::from(new_provider));
        }
    }

    // Update state
    {
        *state.provider_name.lock().await = provider_str.to_string();
        *state.model_name.lock().await = model_str.to_string();
    }

    send_json(ws_tx, &ServerMessage::ModelChanged {
        provider_name: provider_str.to_string(),
        model_name: model_str.to_string(),
    })
    .await;
}

/// Handle config updates (safe subset)
async fn handle_set_config(
    values: serde_json::Value,
    state: &Arc<AppState>,
    ws_tx: &mut futures::stream::SplitSink<WebSocket, Message>,
) {
    let mut config = state.config.lock().await;

    if let Some(max_tokens) = values.get("maxTokens").and_then(|v| v.as_u64()) {
        let clamped = max_tokens.clamp(512, 32768) as u32;
        config.config.max_tokens = clamped;
    }
    if let Some(temp) = values.get("temperature").and_then(|v| v.as_f64()) {
        config.config.temperature = Some(temp.clamp(0.0, 2.0) as f32);
    }
    if let Some(tz) = values.get("timezone").and_then(|v| v.as_str())
        && tz.len() <= 64
    {
        config.config.timezone = Some(tz.to_string());
    }
    if let Some(theme) = values.get("syntaxTheme").and_then(|v| v.as_str())
        && theme.len() <= 64
    {
        config.config.syntax_theme = Some(theme.to_string());
    }

    // Save to disk
    if let Err(e) = lukan_core::config::ConfigManager::save(&config.config).await {
        error!(error = %e, "Failed to save config");
    }

    let safe = serde_json::json!({
        "maxTokens": config.config.max_tokens,
        "temperature": config.config.temperature,
        "timezone": config.config.timezone,
        "syntaxTheme": config.config.syntax_theme,
    });

    send_json(ws_tx, &ServerMessage::ConfigSaved { config: safe }).await;
}

/// Send a ServerMessage as JSON over WebSocket
async fn send_json(
    ws_tx: &mut futures::stream::SplitSink<WebSocket, Message>,
    msg: &ServerMessage,
) {
    use futures::SinkExt;
    if let Ok(json) = serde_json::to_string(msg) {
        let _ = ws_tx.send(Message::Text(json.into())).await;
    }
}

/// Send init state to a newly connected/authenticated client
async fn send_init(
    state: &Arc<AppState>,
    ws_tx: &mut futures::stream::SplitSink<WebSocket, Message>,
) {
    let agent_lock = state.agent.lock().await;
    let provider_name = state.provider_name.lock().await.clone();
    let model_name = state.model_name.lock().await.clone();

    let (session_id, messages, checkpoints, token_usage, context_size) =
        if let Some(ref agent) = *agent_lock {
            (
                agent.session_id().to_string(),
                agent.messages_json(),
                agent.checkpoints().to_vec(),
                TokenUsage {
                    input: agent.input_tokens(),
                    output: agent.output_tokens(),
                    cache_creation: None,
                    cache_read: None,
                },
                agent.last_context_size(),
            )
        } else {
            (
                String::new(),
                vec![],
                vec![],
                TokenUsage {
                    input: 0,
                    output: 0,
                    cache_creation: None,
                    cache_read: None,
                },
                0,
            )
        };

    send_json(ws_tx, &ServerMessage::Init {
        session_id,
        messages,
        checkpoints,
        token_usage,
        context_size,
        permission_mode: "skip".to_string(),
        provider_name,
        model_name,
        browser_screenshots: false,
    })
    .await;
}

/// Create a new AgentLoop
async fn create_agent(state: &Arc<AppState>) -> anyhow::Result<AgentLoop> {
    let config = state.config.lock().await;
    let provider = create_provider(&config)?;
    let system_prompt = build_system_prompt().await;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let provider_name = state.provider_name.lock().await.clone();
    let model_name = state.model_name.lock().await.clone();

    let agent_config = AgentConfig {
        provider: Arc::from(provider),
        tools: create_default_registry(),
        system_prompt,
        cwd,
        provider_name,
        model_name,
        bg_signal: None,
    };

    AgentLoop::new(agent_config).await
}

/// Create an AgentLoop with a loaded session
async fn create_agent_with_session(
    state: &Arc<AppState>,
    session_id: &str,
) -> anyhow::Result<AgentLoop> {
    let config = state.config.lock().await;
    let provider = create_provider(&config)?;
    let system_prompt = build_system_prompt().await;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let provider_name = state.provider_name.lock().await.clone();
    let model_name = state.model_name.lock().await.clone();

    let agent_config = AgentConfig {
        provider: Arc::from(provider),
        tools: create_default_registry(),
        system_prompt,
        cwd,
        provider_name,
        model_name,
        bg_signal: None,
    };

    AgentLoop::load_session(agent_config, session_id).await
}

/// Build system prompt (matches TUI logic)
async fn build_system_prompt() -> SystemPrompt {
    const BASE: &str = include_str!("../../../prompts/base.txt");
    let mut cached = vec![BASE.to_string()];

    // Load global memory
    let global_path = LukanPaths::global_memory_file();
    if let Ok(memory) = tokio::fs::read_to_string(&global_path).await {
        let trimmed = memory.trim();
        if !trimmed.is_empty() {
            cached.push(format!("## Global Memory\n\n{trimmed}"));
        }
    }

    // Load project memory if active
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

    // Load prompt.txt from installed plugins that provide tools
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

    // Dynamic part: current date/time and timezone (changes every call, not cached)
    let now = chrono::Utc::now();
    let tz_name = lukan_core::config::ConfigManager::load()
        .await
        .ok()
        .and_then(|c| c.timezone)
        .unwrap_or_else(|| "UTC".to_string());
    let dynamic = format!(
        "Current date: {} ({}). Use this for any time-relative operations.",
        now.format("%Y-%m-%d %H:%M UTC"),
        tz_name
    );

    SystemPrompt::Structured { cached, dynamic }
}
