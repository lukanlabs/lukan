use lukan_agent::SessionManager;
use lukan_core::config::types::PermissionMode;
use lukan_core::config::{ConfigManager, CredentialsManager, ResolvedConfig};
use lukan_core::models::events::{
    ApprovalResponse, PlanReviewResponse, PlanTask, ToolApprovalRequest,
};
use lukan_core::models::events::StreamEvent;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::mpsc;

use crate::state::ChatState;

// ── Serializable types for the frontend ──────────────────────────────

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

// ── Commands ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn initialize_chat(state: State<'_, ChatState>) -> Result<InitResponse, String> {
    let config = ConfigManager::load().await.map_err(|e| e.to_string())?;
    let credentials = CredentialsManager::load().await.map_err(|e| e.to_string())?;
    let resolved = ResolvedConfig { config, credentials };

    let provider_name = resolved.config.provider.to_string();
    let model_name = resolved.effective_model();

    *state.provider_name.lock().await = provider_name.clone();
    *state.model_name.lock().await = model_name.clone();
    *state.config.lock().await = Some(resolved.clone());

    // Return init info; agent is lazily created on first message
    let agent_lock = state.agent.lock().await;
    let (session_id, messages, token_usage, context_size) = if let Some(ref agent) = *agent_lock {
        (
            agent.session_id().to_string(),
            serde_json::to_value(agent.messages_json()).unwrap_or_default(),
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
            serde_json::Value::Array(vec![]),
            TokenUsage {
                input: 0,
                output: 0,
                cache_creation: None,
                cache_read: None,
            },
            0,
        )
    };

    let permission_mode = state.permission_mode.lock().await.to_string();

    Ok(InitResponse {
        session_id,
        messages,
        provider_name,
        model_name,
        permission_mode,
        token_usage,
        context_size,
    })
}

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, ChatState>,
    content: String,
) -> Result<(), String> {
    // Check if already processing
    {
        let mut processing = state.is_processing.lock().await;
        if *processing {
            return Err("Already processing a message".to_string());
        }
        *processing = true;
    }

    // Ensure agent exists
    {
        let mut agent_lock = state.agent.lock().await;
        if agent_lock.is_none() {
            let config_lock = state.config.lock().await;
            let config = config_lock
                .as_ref()
                .ok_or("Chat not initialized. Call initialize_chat first.")?
                .clone();
            drop(config_lock);

            match state.create_agent(&config).await {
                Ok(agent) => *agent_lock = Some(agent),
                Err(e) => {
                    *state.is_processing.lock().await = false;
                    return Err(format!("Failed to create agent: {e}"));
                }
            }
        }
    }

    // Take agent out of mutex for the turn
    let mut agent = {
        let mut lock = state.agent.lock().await;
        lock.take().unwrap()
    };

    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);
    let content_owned = content;
    let app_handle = app.clone();

    // Spawn agent turn
    let agent_handle = tokio::spawn(async move {
        let result = agent.run_turn(&content_owned, event_tx, None, None).await;
        (agent, result)
    });

    // Spawn event forwarder
    let app_for_events = app.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = app_for_events.emit("stream-event", json);
            }
        }
    });

    // Spawn completion handler — use AppHandle to access state in spawned task
    let app_for_complete = app.clone();
    tokio::spawn(async move {
        match agent_handle.await {
            Ok((returned_agent, result)) => {
                if let Err(e) = result {
                    let _ = app_handle.emit(
                        "stream-event",
                        serde_json::to_string(&serde_json::json!({
                            "type": "error",
                            "error": format!("Agent error: {e}")
                        }))
                        .unwrap_or_default(),
                    );
                }

                let messages = returned_agent.messages_json();
                let context_size = returned_agent.last_context_size();
                let input_tokens = returned_agent.input_tokens();
                let output_tokens = returned_agent.output_tokens();

                let complete = TurnComplete {
                    messages: serde_json::to_value(messages).unwrap_or_default(),
                    context_size,
                    token_usage: TokenUsage {
                        input: input_tokens,
                        output: output_tokens,
                        cache_creation: None,
                        cache_read: None,
                    },
                };

                let _ = app_handle.emit("turn-complete", serde_json::to_string(&complete).unwrap_or_default());

                // Put agent back
                let chat_state = app_for_complete.state::<ChatState>();
                *chat_state.agent.lock().await = Some(returned_agent);
            }
            Err(e) => {
                let _ = app_handle.emit(
                    "stream-event",
                    serde_json::to_string(&serde_json::json!({
                        "type": "error",
                        "error": format!("Agent task failed: {e}")
                    }))
                    .unwrap_or_default(),
                );
            }
        }

        let chat_state = app_for_complete.state::<ChatState>();
        *chat_state.is_processing.lock().await = false;
    });

    Ok(())
}

#[tauri::command]
pub async fn cancel_stream(state: State<'_, ChatState>) -> Result<(), String> {
    // Drop the agent to cancel — the event channel will close and the turn will abort
    let mut agent_lock = state.agent.lock().await;
    *agent_lock = None;
    *state.is_processing.lock().await = false;
    Ok(())
}

#[tauri::command]
pub async fn approve_tools(
    state: State<'_, ChatState>,
    approved_ids: Vec<String>,
) -> Result<(), String> {
    let tx = state.approval_tx.lock().await;
    if let Some(ref sender) = *tx {
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
    approved_ids: Vec<String>,
    tools: Vec<ToolApprovalRequest>,
) -> Result<(), String> {
    let tx = state.approval_tx.lock().await;
    if let Some(ref sender) = *tx {
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
pub async fn deny_all_tools(state: State<'_, ChatState>) -> Result<(), String> {
    let tx = state.approval_tx.lock().await;
    if let Some(ref sender) = *tx {
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
    tasks: Option<serde_json::Value>,
) -> Result<(), String> {
    let modified_tasks: Option<Vec<PlanTask>> =
        tasks.and_then(|v| serde_json::from_value(v).ok());
    let tx = state.plan_review_tx.lock().await;
    if let Some(ref sender) = *tx {
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
    feedback: String,
) -> Result<(), String> {
    let tx = state.plan_review_tx.lock().await;
    if let Some(ref sender) = *tx {
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
    answer: String,
) -> Result<(), String> {
    let tx = state.planner_answer_tx.lock().await;
    if let Some(ref sender) = *tx {
        sender
            .send(answer)
            .await
            .map_err(|e| e.to_string())?;
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
    id: String,
) -> Result<InitResponse, String> {
    // Save current session
    {
        let mut agent_lock = state.agent.lock().await;
        if let Some(ref mut agent) = *agent_lock {
            let _ = agent.save_session_public().await;
        }
    }

    let config_lock = state.config.lock().await;
    let config = config_lock
        .as_ref()
        .ok_or("Chat not initialized")?
        .clone();
    drop(config_lock);

    let agent = state
        .create_agent_with_session(&config, &id)
        .await
        .map_err(|e| format!("Failed to load session: {e}"))?;

    let session_id = agent.session_id().to_string();
    let messages = agent.messages_json();
    let input_tokens = agent.input_tokens();
    let output_tokens = agent.output_tokens();
    let context_size = agent.last_context_size();
    let provider_name = state.provider_name.lock().await.clone();
    let model_name = state.model_name.lock().await.clone();
    let permission_mode = state.permission_mode.lock().await.to_string();

    *state.agent.lock().await = Some(agent);

    Ok(InitResponse {
        session_id,
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
pub async fn new_session(state: State<'_, ChatState>) -> Result<InitResponse, String> {
    // Save current session
    {
        let mut agent_lock = state.agent.lock().await;
        if let Some(ref mut agent) = *agent_lock {
            let _ = agent.save_session_public().await;
        }
        *agent_lock = None;
    }

    let provider_name = state.provider_name.lock().await.clone();
    let model_name = state.model_name.lock().await.clone();
    let permission_mode = state.permission_mode.lock().await.to_string();

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
pub async fn set_permission_mode(
    state: State<'_, ChatState>,
    mode: String,
) -> Result<(), String> {
    let parsed: PermissionMode =
        serde_json::from_value(serde_json::Value::String(mode))
            .unwrap_or(PermissionMode::Auto);

    *state.permission_mode.lock().await = parsed.clone();

    // Update live agent if it exists
    let mut agent_lock = state.agent.lock().await;
    if let Some(ref mut agent) = *agent_lock {
        agent.set_permission_mode(parsed);
    }

    Ok(())
}
