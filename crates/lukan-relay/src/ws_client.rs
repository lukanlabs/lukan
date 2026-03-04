use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{info, warn};

use lukan_core::relay::RelayToDaemon;

use crate::state::{BrowserConnection, RelayState};

/// Handle a browser WebSocket connection.
///
/// The browser must authenticate by sending a JWT token as the first message
/// or via the `?token=` query parameter (already validated before calling this).
pub async fn handle_browser_ws(socket: WebSocket, state: Arc<RelayState>, user_id: String) {
    let connection_id = uuid::Uuid::new_v4().to_string();

    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Register this browser connection
    state.browser_connections.insert(
        connection_id.clone(),
        BrowserConnection {
            user_id: user_id.clone(),
            tx,
        },
    );

    info!(
        connection_id = %connection_id,
        user_id = %user_id,
        "Browser client connected"
    );

    // Tell browser its connection_id (needed for E2E REST tunnel)
    let conn_id_msg = serde_json::json!({ "type": "connection_id", "id": &connection_id });
    state.send_to_browser(&connection_id, &conn_id_msg.to_string());

    // Notify daemon of new browser connection
    let opened_msg = serde_json::to_string(&RelayToDaemon::ConnectionOpened {
        connection_id: connection_id.clone(),
    })
    .unwrap();
    state.send_to_daemon(&user_id, &opened_msg);

    // Spawn writer: relay → browser
    let conn_id_writer = connection_id.clone();
    let writer_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
        let _ = ws_tx.close().await;
        tracing::debug!(connection_id = %conn_id_writer, "Browser WS writer closed");
    });

    // Reader: browser → relay → daemon
    let conn_id_reader = connection_id.clone();
    let user_id_reader = user_id.clone();
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            Message::Text(text) => {
                // Wrap the client message in a relay Forward envelope
                let forward = serde_json::to_string(&RelayToDaemon::Forward {
                    connection_id: conn_id_reader.clone(),
                    message: match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(v) => v,
                        Err(_) => {
                            warn!("Invalid JSON from browser, skipping");
                            continue;
                        }
                    },
                })
                .unwrap();

                if !state.send_to_daemon(&user_id_reader, &forward) {
                    // No daemon connected — send error to browser
                    let err = serde_json::json!({
                        "type": "error",
                        "error": "Your local lukan daemon is not connected. Run `lukan daemon start` and `lukan login` to connect."
                    });
                    state.send_to_browser(&conn_id_reader, &err.to_string());
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    // Cleanup
    state.browser_connections.remove(&connection_id);

    // Notify daemon of closed browser connection
    let closed_msg = serde_json::to_string(&RelayToDaemon::ConnectionClosed {
        connection_id: connection_id.clone(),
    })
    .unwrap();
    state.send_to_daemon(&user_id, &closed_msg);

    writer_task.abort();
    info!(connection_id = %connection_id, "Browser client disconnected");
}

