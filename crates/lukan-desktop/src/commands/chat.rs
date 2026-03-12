use std::sync::Arc;
use std::sync::atomic::Ordering;

use lukan_agent::SessionManager;
use lukan_core::config::types::PermissionMode;
use lukan_core::config::{ConfigManager, CredentialsManager, ResolvedConfig};
use lukan_core::models::events::StreamEvent;
use lukan_core::models::events::{
    ApprovalResponse, PlanReviewResponse, PlanTask, ToolApprovalRequest,
};
use lukan_providers::create_provider;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::state::ChatState;

// ── Serializable types for the frontend ──────────────────────────────

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TaskInfoJs {
    pub id: u32,
    pub title: String,
    pub status: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InitResponse {
    pub session_id: String,
    pub messages: serde_json::Value,
    pub provider_name: String,
    pub model_name: String,
    pub permission_mode: String,
    pub token_usage: TokenUsage,
    pub context_size: u64,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_creation: Option<u64>,
    pub cache_read: Option<u64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TurnComplete {
    pub session_id: String,
    pub messages: serde_json::Value,
    pub context_size: u64,
    pub token_usage: TokenUsage,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryJs {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: usize,
    pub first_user_message: String,
    pub last_user_message: String,
    pub model: String,
}

// ── Tab management commands ──────────────────────────────────────────

#[tauri::command]
pub async fn create_agent_tab(state: State<'_, ChatState>) -> Result<String, String> {
    let tab_id = uuid::Uuid::new_v4().to_string();
    let mut sessions = state.sessions.lock().await;
    let tab_number = sessions.len() + 1;
    let mut session = crate::state::AgentSession::new(tab_id.clone());
    session.label = format!("Agent {tab_number}");
    sessions.insert(tab_id.clone(), session);
    Ok(tab_id)
}

#[tauri::command]
pub async fn destroy_agent_tab(
    state: State<'_, ChatState>,
    session_id: String,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().await;
    if let Some(mut session) = sessions.remove(&session_id) {
        // Cancel any running turn
        session.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut recovered) = session.cancel_running_turn().await {
            let _ = recovered.save_session_public().await;
        }
        // Save current agent session
        if let Some(ref mut agent) = session.agent {
            let _ = agent.save_session_public().await;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn rename_agent_tab(
    state: State<'_, ChatState>,
    session_id: String,
    label: String,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().await;
    if let Some(session) = sessions.get_mut(&session_id) {
        session.label = label.clone();
        // Persist the name to the chat session on disk
        if let Some(ref mut agent) = session.agent {
            let _ = agent.set_session_name(label).await;
        }
    }
    Ok(())
}

// ── Global commands (unchanged) ──────────────────────────────────────

#[tauri::command]
pub async fn initialize_chat(state: State<'_, ChatState>) -> Result<InitResponse, String> {
    let config = ConfigManager::load().await.map_err(|e| e.to_string())?;
    let credentials = CredentialsManager::load()
        .await
        .map_err(|e| e.to_string())?;
    let resolved = ResolvedConfig {
        config,
        credentials,
    };

    let provider_name = resolved.config.provider.to_string();
    let model_name = resolved.effective_model().unwrap_or_default();

    // If provider or model changed, hot-swap on all existing agents
    {
        let old_provider = state.provider_name.lock().await.clone();
        let old_model = state.model_name.lock().await.clone();
        if !old_provider.is_empty() && (old_provider != provider_name || old_model != model_name) {
            let mut sessions = state.sessions.lock().await;
            for session in sessions.values_mut() {
                if let Some(ref mut agent) = session.agent
                    && let Ok(new_provider) = create_provider(&resolved)
                {
                    agent.swap_provider(Arc::from(new_provider));
                }
            }
        }
    }

    *state.provider_name.lock().await = provider_name.clone();
    *state.model_name.lock().await = model_name.clone();
    *state.config.lock().await = Some(resolved.clone());

    let permission_mode = state.permission_mode.borrow().to_string();

    Ok(InitResponse {
        session_id: String::new(),
        messages: serde_json::Value::Array(vec![]),
        provider_name,
        model_name,
        permission_mode,
        token_usage: TokenUsage {
            input: 0,
            output: 0,
            cache_creation: None,
            cache_read: None,
        },
        context_size: 0,
    })
}

// ── Per-session commands ─────────────────────────────────────────────

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, ChatState>,
    session_id: String,
    content: String,
) -> Result<(), String> {
    let stream_event_name = format!("stream-event-{session_id}");
    let turn_complete_event_name = format!("turn-complete-{session_id}");

    // Check if already processing
    {
        let mut sessions = state.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or("Session not found")?;
        if session.is_processing {
            return Err("Already processing a message".to_string());
        }
        session.is_processing = true;
    }

    // Ensure agent exists
    {
        let mut sessions = state.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or("Session not found")?;
        if session.agent.is_none() {
            let config_lock = state.config.lock().await;
            let config = config_lock
                .as_ref()
                .ok_or("Chat not initialized. Call initialize_chat first.")?
                .clone();
            drop(config_lock);

            match state.create_agent(&config).await {
                Ok((agent, channels)) => {
                    session.agent = Some(agent);
                    session.approval_tx = Some(channels.approval_tx);
                    session.plan_review_tx = Some(channels.plan_review_tx);
                    session.planner_answer_tx = Some(channels.planner_answer_tx);
                    session.bg_signal_tx = Some(channels.bg_signal_tx);
                }
                Err(e) => {
                    session.is_processing = false;
                    if config.effective_model().is_none() {
                        let _ = app.emit(
                            &stream_event_name,
                            serde_json::to_string(&serde_json::json!({
                                "type": "text_delta",
                                "text": "No model selected. Go to **Providers** in the sidebar and choose a model to get started."
                            }))
                            .unwrap_or_default(),
                        );
                        let _ = app.emit(
                            &stream_event_name,
                            serde_json::to_string(&serde_json::json!({
                                "type": "message_end",
                                "stop_reason": "end_turn"
                            }))
                            .unwrap_or_default(),
                        );
                        return Ok(());
                    }
                    return Err(format!("Failed to create agent: {e}"));
                }
            }
        }
    }

    // Refresh channels so a reused agent never has stale receivers
    {
        let mut sessions = state.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or("Session not found")?;
        if let Some(mut agent) = session.agent.take() {
            session.refresh_channels(&mut agent);
            session.agent = Some(agent);
        }
    }

    // Take agent out of session for the turn
    let mut agent = {
        let mut sessions = state.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or("Session not found")?;
        let mut a = session.agent.take().unwrap();
        a.label = Some(session.label.clone());
        a.tab_id = Some(session_id.clone());
        a
    };

    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);
    let content_owned = content;
    let app_handle = app.clone();
    let session_id_owned = session_id.clone();

    // Bump generation so stale completion handlers don't overwrite state
    let turn_gen = {
        let sessions = state.sessions.lock().await;
        let session = sessions.get(&session_id).ok_or("Session not found")?;
        session.generation.fetch_add(1, Ordering::SeqCst) + 1
    };

    // Create cancellation token
    let cancel_token = CancellationToken::new();
    {
        let mut sessions = state.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or("Session not found")?;
        session.cancel_token = Some(cancel_token.clone());
    }

    // Spawn agent turn
    let agent_handle = tokio::spawn(async move {
        let result = agent
            .run_turn(&content_owned, event_tx, Some(cancel_token), None)
            .await;
        (agent, result)
    });

    // Store handle
    {
        let mut sessions = state.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or("Session not found")?;
        session.agent_handle = Some(agent_handle);
    }

    // Spawn event forwarder (scoped to this tab)
    let stream_event_name_clone = stream_event_name.clone();
    let app_for_events = app.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = app_for_events.emit(&stream_event_name_clone, json);
            }
        }
    });

    // Spawn completion handler
    let app_for_complete = app.clone();
    tokio::spawn(async move {
        let handle = {
            let chat_state = app_for_complete.state::<ChatState>();
            let mut sessions = chat_state.sessions.lock().await;
            sessions
                .get_mut(&session_id_owned)
                .and_then(|s| s.agent_handle.take())
        };

        if let Some(handle) = handle {
            match handle.await {
                Ok((mut returned_agent, result)) => {
                    if let Err(e) = result {
                        let _ = app_handle.emit(
                            &stream_event_name,
                            serde_json::to_string(&serde_json::json!({
                                "type": "error",
                                "error": format!("Agent error: {e}")
                            }))
                            .unwrap_or_default(),
                        );
                    }

                    let chat_state = app_for_complete.state::<ChatState>();
                    let mut sessions = chat_state.sessions.lock().await;

                    if let Some(session) = sessions.get_mut(&session_id_owned) {
                        let current_gen = session.generation.load(Ordering::SeqCst);
                        if current_gen != turn_gen {
                            let _ = returned_agent.save_session_public().await;
                            return;
                        }

                        let agent_session_id = returned_agent.session_id().to_string();
                        let messages = returned_agent.messages_json();
                        let context_size = returned_agent.last_context_size();
                        let input_tokens = returned_agent.input_tokens();
                        let output_tokens = returned_agent.output_tokens();

                        let complete = TurnComplete {
                            session_id: agent_session_id,
                            messages: serde_json::to_value(messages).unwrap_or_default(),
                            context_size,
                            token_usage: TokenUsage {
                                input: input_tokens,
                                output: output_tokens,
                                cache_creation: None,
                                cache_read: None,
                            },
                        };

                        let _ = app_handle.emit(
                            &turn_complete_event_name,
                            serde_json::to_string(&complete).unwrap_or_default(),
                        );

                        session.agent = Some(returned_agent);
                        session.cancel_token = None;
                        session.is_processing = false;
                    } else {
                        // Session was destroyed while turn was running — just save
                        let _ = returned_agent.save_session_public().await;
                    }
                }
                Err(e) => {
                    if !e.is_cancelled() {
                        let _ = app_handle.emit(
                            &stream_event_name,
                            serde_json::to_string(&serde_json::json!({
                                "type": "error",
                                "error": format!("Agent task failed: {e}")
                            }))
                            .unwrap_or_default(),
                        );
                    }
                    let chat_state = app_for_complete.state::<ChatState>();
                    let mut sessions = chat_state.sessions.lock().await;
                    if let Some(session) = sessions.get_mut(&session_id_owned) {
                        let current_gen = session.generation.load(Ordering::SeqCst);
                        if current_gen == turn_gen {
                            session.cancel_token = None;
                            session.is_processing = false;
                        }
                    }
                }
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_stream(
    app: AppHandle,
    state: State<'_, ChatState>,
    session_id: String,
) -> Result<(), String> {
    let (cancel_token, agent_handle) = {
        let mut sessions = state.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or("Session not found")?;
        (session.cancel_token.take(), session.agent_handle.take())
    };

    // Signal cancellation
    if let Some(token) = cancel_token {
        token.cancel();
    }

    if let Some(mut handle) = agent_handle {
        let session_id_clone = session_id.clone();
        tokio::select! {
            result = &mut handle => {
                match result {
                    Ok((mut returned_agent, _result)) => {
                        let _ = returned_agent.save_session_public().await;
                        let mut sessions = state.sessions.lock().await;
                        if let Some(session) = sessions.get_mut(&session_id_clone) {
                            session.agent = Some(returned_agent);
                        }
                    }
                    Err(_join_err) => {}
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                eprintln!("[warn] Agent did not stop within 10s after cancel, recovering in background");
                let session_id_bg = session_id_clone.clone();
                tokio::spawn(async move {
                    if let Ok((mut returned_agent, _)) = handle.await {
                        let _ = returned_agent.save_session_public().await;
                        let chat_state = app.state::<ChatState>();
                        let mut sessions = chat_state.sessions.lock().await;
                        if let Some(session) = sessions.get_mut(&session_id_bg) {
                            session.agent = Some(returned_agent);
                        }
                    }
                });
            }
        }
    }

    {
        let mut sessions = state.sessions.lock().await;
        if let Some(session) = sessions.get_mut(&session_id) {
            session.is_processing = false;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn approve_tools(
    state: State<'_, ChatState>,
    session_id: String,
    approved_ids: Vec<String>,
) -> Result<(), String> {
    let sessions = state.sessions.lock().await;
    let session = sessions.get(&session_id).ok_or("Session not found")?;
    if let Some(ref sender) = session.approval_tx {
        sender
            .send(ApprovalResponse::Approved { approved_ids })
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn always_allow_tools(
    state: State<'_, ChatState>,
    session_id: String,
    approved_ids: Vec<String>,
    tools: Vec<ToolApprovalRequest>,
) -> Result<(), String> {
    let sessions = state.sessions.lock().await;
    let session = sessions.get(&session_id).ok_or("Session not found")?;
    if let Some(ref sender) = session.approval_tx {
        sender
            .send(ApprovalResponse::AlwaysAllow {
                approved_ids,
                tools,
            })
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn deny_all_tools(state: State<'_, ChatState>, session_id: String) -> Result<(), String> {
    let sessions = state.sessions.lock().await;
    let session = sessions.get(&session_id).ok_or("Session not found")?;
    if let Some(ref sender) = session.approval_tx {
        sender
            .send(ApprovalResponse::DeniedAll)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn accept_plan(
    state: State<'_, ChatState>,
    session_id: String,
    tasks: Option<serde_json::Value>,
) -> Result<(), String> {
    let modified_tasks: Option<Vec<PlanTask>> = tasks.and_then(|v| serde_json::from_value(v).ok());
    let sessions = state.sessions.lock().await;
    let session = sessions.get(&session_id).ok_or("Session not found")?;
    if let Some(ref sender) = session.plan_review_tx {
        sender
            .send(PlanReviewResponse::Accepted { modified_tasks })
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn reject_plan(
    state: State<'_, ChatState>,
    session_id: String,
    feedback: String,
) -> Result<(), String> {
    let sessions = state.sessions.lock().await;
    let session = sessions.get(&session_id).ok_or("Session not found")?;
    if let Some(ref sender) = session.plan_review_tx {
        sender
            .send(PlanReviewResponse::Rejected { feedback })
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn answer_question(
    state: State<'_, ChatState>,
    session_id: String,
    answer: String,
) -> Result<(), String> {
    let sessions = state.sessions.lock().await;
    let session = sessions.get(&session_id).ok_or("Session not found")?;
    if let Some(ref sender) = session.planner_answer_tx {
        sender.send(answer).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn list_sessions() -> Result<Vec<SessionSummaryJs>, String> {
    let sessions = SessionManager::list().await.map_err(|e| e.to_string())?;
    Ok(sessions
        .into_iter()
        .map(|s| SessionSummaryJs {
            id: s.id,
            name: s.name.unwrap_or_default(),
            created_at: s.created_at.to_rfc3339(),
            updated_at: s.updated_at.to_rfc3339(),
            message_count: s.message_count,
            first_user_message: s.last_message.clone().unwrap_or_default(),
            last_user_message: s.last_message.unwrap_or_default(),
            model: s.model.unwrap_or_default(),
        })
        .collect())
}

#[tauri::command]
pub async fn load_session(
    state: State<'_, ChatState>,
    session_id: String,
    id: String,
) -> Result<InitResponse, String> {
    {
        let mut sessions = state.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or("Session not found")?;

        // Cancel any running turn and recover the agent
        session.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut recovered) = session.cancel_running_turn().await {
            let _ = recovered.save_session_public().await;
        }

        // Save current session if agent exists
        if let Some(ref mut agent) = session.agent {
            let _ = agent.save_session_public().await;
        }
        session.agent = None;
    }

    let config_lock = state.config.lock().await;
    let config = config_lock.as_ref().ok_or("Chat not initialized")?.clone();
    drop(config_lock);

    let (agent, channels) = state
        .create_agent_with_session(&config, &id)
        .await
        .map_err(|e| format!("Failed to load session: {e}"))?;

    let agent_session_id = agent.session_id().to_string();
    let messages = agent.messages_json();
    let input_tokens = agent.input_tokens();
    let output_tokens = agent.output_tokens();
    let context_size = agent.last_context_size();
    let provider_name = state.provider_name.lock().await.clone();
    let model_name = state.model_name.lock().await.clone();
    let permission_mode = state.permission_mode.borrow().to_string();

    {
        let mut sessions = state.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or("Session not found")?;
        session.agent = Some(agent);
        session.approval_tx = Some(channels.approval_tx);
        session.plan_review_tx = Some(channels.plan_review_tx);
        session.planner_answer_tx = Some(channels.planner_answer_tx);
        session.bg_signal_tx = Some(channels.bg_signal_tx);
    }

    Ok(InitResponse {
        session_id: agent_session_id,
        messages: serde_json::to_value(messages).unwrap_or_default(),
        provider_name,
        model_name,
        permission_mode,
        token_usage: TokenUsage {
            input: input_tokens,
            output: output_tokens,
            cache_creation: None,
            cache_read: None,
        },
        context_size,
    })
}

#[tauri::command]
pub async fn new_session(
    state: State<'_, ChatState>,
    session_id: String,
) -> Result<InitResponse, String> {
    {
        let mut sessions = state.sessions.lock().await;
        let session = sessions.get_mut(&session_id).ok_or("Session not found")?;

        // Cancel any running turn
        session.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut recovered) = session.cancel_running_turn().await {
            let _ = recovered.save_session_public().await;
        }

        // Save current session
        if let Some(ref mut agent) = session.agent {
            let _ = agent.save_session_public().await;
        }
        session.agent = None;
    }

    let provider_name = state.provider_name.lock().await.clone();
    let model_name = state.model_name.lock().await.clone();
    let permission_mode = state.permission_mode.borrow().to_string();

    Ok(InitResponse {
        session_id: String::new(),
        messages: serde_json::Value::Array(vec![]),
        provider_name,
        model_name,
        permission_mode,
        token_usage: TokenUsage {
            input: 0,
            output: 0,
            cache_creation: None,
            cache_read: None,
        },
        context_size: 0,
    })
}

#[tauri::command]
pub async fn set_permission_mode(state: State<'_, ChatState>, mode: String) -> Result<(), String> {
    let parsed: PermissionMode =
        serde_json::from_value(serde_json::Value::String(mode)).unwrap_or(PermissionMode::Auto);

    let _ = state.permission_mode.send(parsed);

    Ok(())
}

#[tauri::command]
pub async fn list_tasks() -> Result<Vec<TaskInfoJs>, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let tasks = lukan_tools::tasks::read_all_tasks(&cwd).await;
    Ok(tasks
        .iter()
        .map(|t| TaskInfoJs {
            id: t.id,
            title: t.title.clone(),
            status: t.status.label().to_string(),
        })
        .collect())
}
