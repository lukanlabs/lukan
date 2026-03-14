//! WebSocket client for connecting the TUI to the daemon.
//!
//! Speaks the same ClientMessage/ServerMessage protocol as the web UI.
//! Converts daemon messages into `StreamEvent` that the TUI already knows how to render.

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tracing::{info, warn};

use lukan_core::models::events::{StreamEvent, ToolApprovalRequest};
use lukan_core::models::messages::Message;

/// Messages we send to the daemon (mirrors lukan-web's ClientMessage).
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum OutMessage {
    SendMessage {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    Approve {
        approved_ids: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    AlwaysAllow {
        approved_ids: Vec<String>,
        tools: Vec<ToolApprovalRequest>,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    DenyAll {
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    Abort {
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    PlanAccept {
        tasks: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    PlanReject {
        feedback: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    PlanTaskFeedback {
        task_index: u32,
        feedback: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    AnswerQuestion {
        answer: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    LoadSession {
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    NewSession {
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    ListSessions,
    CreateAgentTab,
    SetPermissionMode {
        mode: String,
    },
    SendToBackground {
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
}

/// Events received from the daemon, converted to TUI-consumable form.
#[derive(Debug)]
pub enum DaemonEvent {
    /// A stream event from the agent (text delta, tool use, approval, etc.)
    Stream(StreamEvent),
    /// Initialization data from the daemon (session state, provider info)
    Init {
        session_id: String,
        messages: Vec<Message>,
        provider_name: String,
        model_name: String,
        context_size: u64,
    },
    /// Agent tab created — contains the tab_id to use for all messages
    TabCreated { tab_id: String },
    /// Processing complete with session state
    ProcessingComplete {
        session_id: String,
        context_size: Option<u64>,
        aborted: bool,
    },
    /// Session list response
    SessionList {
        sessions: Vec<lukan_core::models::sessions::SessionSummary>,
    },
    /// Session loaded with messages
    SessionLoaded {
        session_id: String,
        messages: Vec<Message>,
        context_size: u64,
    },
    /// Model changed on the daemon
    ModelChanged {
        provider_name: String,
        model_name: String,
    },
    /// Error from the daemon
    Error(String),
    /// Connection lost
    Disconnected,
}

/// Handle to send messages to the daemon.
#[derive(Clone)]
pub struct DaemonSender {
    tx: mpsc::UnboundedSender<String>,
}

impl DaemonSender {
    /// Send a message to the daemon.
    pub fn send(&self, msg: &OutMessage) -> Result<()> {
        let json = serde_json::to_string(msg)?;
        self.tx
            .send(json)
            .map_err(|_| anyhow::anyhow!("Daemon connection closed"))
    }
}

/// Connect to the daemon's WebSocket server.
/// Creates an agent tab on the daemon and returns the tab_id.
///
/// Returns `(sender, receiver, tab_id)`.
pub async fn connect(
    port: u16,
) -> Result<(DaemonSender, mpsc::UnboundedReceiver<DaemonEvent>, String)> {
    let url = format!("ws://127.0.0.1:{port}/ws");
    info!(url, "Connecting to daemon WebSocket");

    let request = tungstenite::http::Request::builder()
        .uri(&url)
        .header("x-relay-internal", "true") // skip auth for local connections
        .header(
            "sec-websocket-key",
            tungstenite::handshake::client::generate_key(),
        )
        .header("sec-websocket-version", "13")
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("host", format!("127.0.0.1:{port}"))
        .body(())
        .context("Failed to build WebSocket request")?;

    let (ws_stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .context("Failed to connect to daemon")?;

    info!("Connected to daemon");

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Send CreateAgentTab immediately to get a tab_id
    let create_msg = serde_json::to_string(&OutMessage::CreateAgentTab)?;
    ws_tx
        .send(tungstenite::Message::Text(create_msg.into()))
        .await
        .context("Failed to send CreateAgentTab")?;

    // Wait for agent_tab_created response (with timeout)
    let tab_id = {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut found_tab_id = None;

        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_secs(5), ws_rx.next()).await {
                Ok(Some(Ok(tungstenite::Message::Text(text)))) => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                        let msg_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        if msg_type == "agent_tab_created" {
                            found_tab_id = value
                                .get("sessionId")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());
                            break;
                        }
                        // Skip init and other setup messages
                    }
                }
                _ => break,
            }
        }

        found_tab_id.context("Did not receive agent_tab_created from daemon")?
    };

    info!(tab_id = %tab_id, "Agent tab created on daemon");

    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<DaemonEvent>();

    // Writer task: sends outgoing messages
    tokio::spawn(async move {
        while let Some(json) = out_rx.recv().await {
            if ws_tx
                .send(tungstenite::Message::Text(json.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Reader task: receives and dispatches incoming messages
    let event_tx_clone = event_tx.clone();
    tokio::spawn(async move {
        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(tungstenite::Message::Text(text)) => {
                    dispatch_message(&text, &event_tx_clone);
                }
                Ok(tungstenite::Message::Close(_)) | Err(_) => {
                    let _ = event_tx_clone.send(DaemonEvent::Disconnected);
                    break;
                }
                _ => {}
            }
        }
    });

    Ok((DaemonSender { tx: out_tx }, event_rx, tab_id))
}

/// Parse an incoming WebSocket message and dispatch it as a DaemonEvent.
fn dispatch_message(text: &str, tx: &mpsc::UnboundedSender<DaemonEvent>) {
    let value: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "Invalid daemon message");
            return;
        }
    };

    let msg_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        // ── ServerMessage types ──
        "init" => {
            let session_id = value
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let messages: Vec<Message> = value
                .get("messages")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            let provider_name = value
                .get("providerName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let model_name = value
                .get("modelName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let context_size = value
                .get("contextSize")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let _ = tx.send(DaemonEvent::Init {
                session_id,
                messages,
                provider_name,
                model_name,
                context_size,
            });
        }
        "agent_tab_created" => {
            let tab_id = value
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let _ = tx.send(DaemonEvent::TabCreated { tab_id });
        }
        "processing_complete" => {
            let session_id = value
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let context_size = value.get("contextSize").and_then(|v| v.as_u64());
            let aborted = value
                .get("aborted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let _ = tx.send(DaemonEvent::ProcessingComplete {
                session_id,
                context_size,
                aborted,
            });
        }
        "session_list" => {
            if let Ok(sessions) =
                serde_json::from_value(value.get("sessions").cloned().unwrap_or_default())
            {
                let _ = tx.send(DaemonEvent::SessionList { sessions });
            }
        }
        "session_loaded" => {
            let session_id = value
                .get("sessionId")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let messages: Vec<Message> = value
                .get("messages")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default();
            let context_size = value
                .get("contextSize")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let _ = tx.send(DaemonEvent::SessionLoaded {
                session_id,
                messages,
                context_size,
            });
        }
        "model_changed" => {
            let provider_name = value
                .get("providerName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let model_name = value
                .get("modelName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let _ = tx.send(DaemonEvent::ModelChanged {
                provider_name,
                model_name,
            });
        }
        "error" => {
            let error = value
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            let _ = tx.send(DaemonEvent::Error(error));
        }
        // ── Ignore server housekeeping ──
        "auth_required" | "auth_ok" | "auth_error" | "config_values" | "config_saved"
        | "workers_update" | "worker_detail" | "worker_run_detail" | "worker_notification"
        | "sub_agents_update" | "model_list" | "agent_tabs_loaded" | "agent_tabs_saved"
        | "terminal_created" | "terminal_sessions" | "terminal_output" | "terminal_exited"
        | "screenshots_changed" | "mode_changed" => {}

        // ── StreamEvent types — deserialize and forward ──
        _ => match serde_json::from_str::<StreamEvent>(text) {
            Ok(ev) => {
                let _ = tx.send(DaemonEvent::Stream(ev));
            }
            Err(_) => {
                // Unknown message type — ignore silently
            }
        },
    }
}
