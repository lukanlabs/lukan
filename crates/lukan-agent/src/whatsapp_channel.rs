use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use lukan_core::config::WhatsAppConfig;
use lukan_core::models::events::StreamEvent;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tracing::{error, info, warn};

use crate::AgentConfig;
use crate::AgentLoop;

// ── Connector protocol types ────────────────────────────────────────────

/// Events received from the whatsapp-connector WebSocket
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ConnectorEvent {
    #[serde(rename = "message")]
    Message {
        sender: String,
        #[serde(rename = "chatId")]
        chat_id: String,
        content: String,
        #[serde(rename = "isGroup")]
        is_group: bool,
    },
    #[serde(rename = "status")]
    Status { status: String },
    #[serde(rename = "groups")]
    Groups { groups: Vec<GroupInfo> },
    #[serde(rename = "audio")]
    Audio {
        sender: String,
        #[serde(rename = "chatId")]
        chat_id: String,
        #[serde(rename = "audioBase64")]
        _audio_base64: String,
        seconds: u32,
        #[serde(rename = "isGroup")]
        is_group: bool,
    },
}

#[derive(Debug, Deserialize)]
pub struct GroupInfo {
    pub id: String,
    pub subject: String,
    pub participants: u32,
}

/// Commands sent to the connector
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
enum ConnectorCommand {
    #[serde(rename = "send")]
    Send { to: String, text: String },
    #[serde(rename = "list_groups")]
    ListGroups,
}

type WsWriter = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, WsMessage>;
type WsReader = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

// ── Max response length ─────────────────────────────────────────────────

const MAX_RESPONSE_LEN: usize = 4000;

// ── WhatsApp channel entry point ────────────────────────────────────────

/// Start the WhatsApp channel, connecting to the connector and routing messages.
pub async fn start_whatsapp_channel(config: AgentConfig, wa_config: &WhatsAppConfig) -> Result<()> {
    let connector_url = wa_config
        .bridge_url
        .clone()
        .unwrap_or_else(|| "ws://localhost:3001".to_string());
    let whitelist: Vec<String> = wa_config.whitelist.clone().unwrap_or_default();
    let allowed_groups: Vec<String> = wa_config.allowed_groups.clone().unwrap_or_default();
    let prefix = wa_config.prefix.clone();

    info!(
        connector_url = %connector_url,
        whitelist_count = whitelist.len(),
        groups_count = allowed_groups.len(),
        "Starting WhatsApp channel"
    );

    // Create the agent loop
    let mut agent = AgentLoop::new(config).await?;
    let processing: Arc<tokio::sync::Mutex<HashSet<String>>> =
        Arc::new(tokio::sync::Mutex::new(HashSet::new()));

    // Connect with auto-reconnect loop
    loop {
        match connect_async(&connector_url).await {
            Ok((ws_stream, _)) => {
                info!("Connected to whatsapp-connector at {}", connector_url);
                let (writer, reader) = ws_stream.split();
                let writer = Arc::new(tokio::sync::Mutex::new(writer));

                if let Err(e) = handle_connection(
                    reader,
                    writer,
                    &mut agent,
                    &whitelist,
                    &allowed_groups,
                    &prefix,
                    &processing,
                )
                .await
                {
                    warn!("Connection handler error: {e}");
                }

                info!("Disconnected from connector, reconnecting in 3s...");
            }
            Err(e) => {
                warn!("Failed to connect to connector: {e}, retrying in 3s...");
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

/// Handle a single WebSocket connection to the connector.
async fn handle_connection(
    mut reader: WsReader,
    writer: Arc<tokio::sync::Mutex<WsWriter>>,
    agent: &mut AgentLoop,
    whitelist: &[String],
    allowed_groups: &[String],
    prefix: &Option<String>,
    processing: &Arc<tokio::sync::Mutex<HashSet<String>>>,
) -> Result<()> {
    while let Some(msg) = reader.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                error!("WebSocket read error: {e}");
                return Err(e.into());
            }
        };

        let text = match msg {
            WsMessage::Text(t) => t.to_string(),
            WsMessage::Close(_) => {
                info!("Connector closed the connection");
                return Ok(());
            }
            _ => continue,
        };

        let event: ConnectorEvent = match serde_json::from_str(&text) {
            Ok(e) => e,
            Err(e) => {
                warn!("Failed to parse connector event: {e}");
                continue;
            }
        };

        match event {
            ConnectorEvent::Message {
                sender,
                chat_id,
                content,
                is_group,
            } => {
                if !should_process(
                    &sender,
                    &chat_id,
                    is_group,
                    whitelist,
                    allowed_groups,
                    prefix,
                ) {
                    continue;
                }

                let message = strip_prefix(&content, prefix);

                // Check if already processing this chat
                {
                    let mut proc = processing.lock().await;
                    if proc.contains(&chat_id) {
                        info!(chat_id = %chat_id, "Already processing, skipping");
                        continue;
                    }
                    proc.insert(chat_id.clone());
                }

                info!(
                    sender = %sender,
                    chat_id = %chat_id,
                    is_group,
                    "Processing message"
                );

                let response = collect_agent_response(agent, &message).await;

                // Send response
                if !response.is_empty() {
                    let cmd = ConnectorCommand::Send {
                        to: chat_id.clone(),
                        text: response,
                    };
                    if let Ok(json) = serde_json::to_string(&cmd) {
                        let mut w = writer.lock().await;
                        if let Err(e) = w.send(WsMessage::Text(json.into())).await {
                            error!("Failed to send response: {e}");
                        }
                    }
                }

                // Remove from processing set
                processing.lock().await.remove(&chat_id);
            }
            ConnectorEvent::Audio {
                sender,
                chat_id,
                seconds,
                is_group,
                ..
            } => {
                if !should_process(
                    &sender,
                    &chat_id,
                    is_group,
                    whitelist,
                    allowed_groups,
                    prefix,
                ) {
                    continue;
                }

                info!(
                    sender = %sender,
                    chat_id = %chat_id,
                    seconds,
                    "Received audio (transcription not supported in Rust yet)"
                );

                // Send a reply indicating audio is not supported yet
                let cmd = ConnectorCommand::Send {
                    to: chat_id,
                    text: "Audio messages are not supported yet. Please send a text message."
                        .to_string(),
                };
                if let Ok(json) = serde_json::to_string(&cmd) {
                    let mut w = writer.lock().await;
                    let _ = w.send(WsMessage::Text(json.into())).await;
                }
            }
            ConnectorEvent::Status { status } => {
                info!(status = %status, "Connector status update");
            }
            ConnectorEvent::Groups { groups } => {
                info!(count = groups.len(), "Received groups list");
            }
        }
    }

    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Check if a message should be processed based on whitelist/groups/prefix.
fn should_process(
    sender: &str,
    chat_id: &str,
    is_group: bool,
    whitelist: &[String],
    allowed_groups: &[String],
    _prefix: &Option<String>,
) -> bool {
    // Default-deny: require at least one whitelist or group
    if whitelist.is_empty() && allowed_groups.is_empty() {
        return false;
    }

    if is_group {
        if !allowed_groups.iter().any(|g| g == chat_id) {
            return false;
        }
    } else if !whitelist.iter().any(|w| w == sender) {
        return false;
    }

    true
}

/// Strip the command prefix from a message, if present.
fn strip_prefix(content: &str, prefix: &Option<String>) -> String {
    if let Some(pfx) = prefix {
        let trimmed = content.trim();
        if let Some(rest) = trimmed.strip_prefix(pfx.as_str()) {
            return rest.trim().to_string();
        }
    }
    content.to_string()
}

/// Run the agent loop for a single message and collect the text response.
/// Resets accumulated text on each ToolResult (only keeps final text).
/// Caps at MAX_RESPONSE_LEN characters.
async fn collect_agent_response(agent: &mut AgentLoop, message: &str) -> String {
    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);

    // run_turn takes ownership of event_tx. When it returns, the sender drops,
    // so recv().await will return None once all buffered events are consumed.
    let turn_result = agent.run_turn(message, event_tx, None).await;

    if let Err(e) = turn_result {
        error!("Agent turn error: {e}");
        return format!("Error: {e}");
    }

    // Drain all events. recv().await blocks until an event arrives or the
    // sender is dropped (returns None), ensuring we collect everything.
    let mut response = String::new();

    while let Some(event) = event_rx.recv().await {
        match event {
            StreamEvent::TextDelta { text } => {
                response.push_str(&text);
            }
            StreamEvent::ToolResult { .. } => {
                // Reset — only keep text after the last tool result
                response.clear();
            }
            _ => {}
        }
    }

    // Truncate if too long
    if response.len() > MAX_RESPONSE_LEN {
        response.truncate(MAX_RESPONSE_LEN);
        response.push_str("... (truncated)");
    }

    if response.is_empty() {
        "(No response)".to_string()
    } else {
        response
    }
}
