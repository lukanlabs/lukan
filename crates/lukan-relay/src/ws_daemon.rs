use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{info, warn};

use lukan_core::relay::DaemonToRelay;

use crate::state::{DaemonConnection, RelayState};

/// Handle a daemon WebSocket connection.
///
/// The daemon authenticates via JWT in the `?token=` query parameter.
/// After connecting, the daemon sends a `Register` message and then
/// begins forwarding messages between the relay and the agent loop.
pub async fn handle_daemon_ws(socket: WebSocket, state: Arc<RelayState>, user_id: String) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    info!(user_id = %user_id, "Daemon connected");

    // Register daemon connection (replaces any existing one for this user)
    state.daemon_connections.insert(
        user_id.clone(),
        DaemonConnection {
            user_id: user_id.clone(),
            device_name: "unknown".into(),
            tx,
        },
    );

    // Spawn writer: relay → daemon
    let user_id_writer = user_id.clone();
    let writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
        let _ = ws_tx.close().await;
        tracing::debug!(user_id = %user_id_writer, "Daemon WS writer closed");
    });

    // Reader: daemon → relay → browser(s)
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                handle_daemon_message(&state, &user_id, &text);
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    // Cleanup: remove daemon connection
    state.daemon_connections.remove(&user_id);
    writer_task.abort();

    // Notify all browser connections for this user that daemon is gone
    let browser_ids: Vec<String> = state
        .browser_connections
        .iter()
        .filter(|entry| entry.value().user_id == user_id)
        .map(|entry| entry.key().clone())
        .collect();

    for conn_id in browser_ids {
        let err = serde_json::json!({
            "type": "error",
            "error": "Your local lukan daemon disconnected."
        });
        state.send_to_browser(&conn_id, &err.to_string());
    }

    info!(user_id = %user_id, "Daemon disconnected");
}

/// Process a message from the daemon.
fn handle_daemon_message(state: &RelayState, user_id: &str, text: &str) {
    let msg: DaemonToRelay = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!(user_id = %user_id, error = %e, "Invalid message from daemon");
            return;
        }
    };

    match msg {
        DaemonToRelay::Register {
            user_id: _,
            device_name,
        } => {
            // Update device name
            if let Some(mut conn) = state.daemon_connections.get_mut(user_id) {
                conn.device_name = device_name.clone();
            }
            info!(user_id = %user_id, device = %device_name, "Daemon registered");
        }
        DaemonToRelay::Forward {
            connection_id,
            message,
        } => {
            // Route server message to the specific browser connection,
            // but ONLY if that connection belongs to the same user.
            // This prevents a compromised daemon from sending to another user's browser.
            let json = serde_json::to_string(&message).unwrap_or_default();
            if !state.send_to_browser_if_owned(&connection_id, user_id, &json) {
                warn!(
                    connection_id = %connection_id,
                    "Browser connection not found or not owned by this user"
                );
            }
        }
        DaemonToRelay::RestResponse {
            request_id,
            status,
            headers,
            body,
        } => {
            // Complete the pending REST tunnel request
            if let Some((_, pending)) = state.pending_rest.remove(&request_id) {
                let _ = pending.tx.send(crate::state::RestTunnelResponse {
                    status,
                    headers,
                    body,
                });
            } else {
                warn!(request_id = %request_id, "No pending REST request for response");
            }
        }
        DaemonToRelay::Ping => {
            // Respond with pong via the daemon connection
            if let Some(conn) = state.daemon_connections.get(user_id) {
                let pong = serde_json::json!({"type": "pong"});
                let _ = conn.tx.send(pong.to_string());
            }
        }
    }
}
