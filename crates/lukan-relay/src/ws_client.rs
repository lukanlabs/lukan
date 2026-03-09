use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{info, warn};

use lukan_core::relay::RelayToDaemon;

use crate::state::{BrowserConnection, RelayState};

/// Handle a browser WebSocket connection.
///
/// The browser authenticates via the HttpOnly `lukan_token` cookie
/// (validated during the WebSocket upgrade in main.rs).
pub async fn handle_browser_ws(
    socket: WebSocket,
    state: Arc<RelayState>,
    user_id: String,
    device_name: String,
    ip_address: String,
) {
    let connection_id = uuid::Uuid::new_v4().to_string();

    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Register this browser connection
    state.browser_connections.insert(
        connection_id.clone(),
        BrowserConnection {
            user_id: user_id.clone(),
            device_name: device_name.clone(),
            tx,
            ip_address,
            connected_at: tokio::time::Instant::now(),
        },
    );

    info!(
        connection_id = %connection_id,
        user_id = %user_id,
        device = %device_name,
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
    state.send_to_daemon(&user_id, &device_name, &opened_msg);

    // Spawn writer: relay → browser (with periodic pings to keep Cloudflare alive)
    let conn_id_writer = connection_id.clone();
    let writer_task = tokio::spawn(async move {
        let mut ping_interval = tokio::time::interval(std::time::Duration::from_secs(30));
        ping_interval.tick().await; // skip first tick
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(text) => {
                            if ws_tx.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                _ = ping_interval.tick() => {
                    if ws_tx.send(Message::Ping(vec![].into())).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = ws_tx.close().await;
        tracing::debug!(connection_id = %conn_id_writer, "Browser WS writer closed");
    });

    // Reader: browser → relay → daemon
    let conn_id_reader = connection_id.clone();
    let user_id_reader = user_id.clone();
    while let Some(result) = ws_rx.next().await {
        let msg = match result {
            Ok(m) => m,
            Err(e) => {
                info!(connection_id = %conn_id_reader, error = %e, "Browser WS read error");
                break;
            }
        };
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

                if !state.send_to_daemon(&user_id_reader, &device_name, &forward) {
                    let err = serde_json::json!({
                        "type": "error",
                        "error": "Your local lukan daemon is not connected. Run `lukan daemon start` and `lukan login` to connect."
                    });
                    state.send_to_browser(&conn_id_reader, &err.to_string());
                }
            }
            Message::Close(frame) => {
                info!(connection_id = %conn_id_reader, close_frame = ?frame, "Browser sent Close frame");
                break;
            }
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
    state.send_to_daemon(&user_id, &device_name, &closed_msg);

    writer_task.abort();
    info!(connection_id = %connection_id, "Browser client disconnected");
}
