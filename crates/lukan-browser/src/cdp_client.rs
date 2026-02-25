//! CDP (Chrome DevTools Protocol) WebSocket client.
//!
//! JSON-RPC over WebSocket:
//!   Request:  `{ id, method, params }`
//!   Response: `{ id, result/error }`
//!   Event:    `{ method, params }`

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tracing::{debug, error, warn};

const SEND_TIMEOUT: Duration = Duration::from_secs(30);

type WsSink = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, WsMessage>;
type WsStream = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

/// A pending CDP request waiting for its response.
type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value>>>>>;

/// Event subscribers: method -> list of senders.
type EventMap = Arc<Mutex<HashMap<String, Vec<mpsc::UnboundedSender<Value>>>>>;

pub struct CdpClient {
    ws_tx: Arc<Mutex<WsSink>>,
    next_id: Arc<Mutex<u64>>,
    pending: PendingMap,
    events: EventMap,
    reader_handle: Option<JoinHandle<()>>,
    connected: Arc<Mutex<bool>>,
}

impl CdpClient {
    /// Connect to a CDP target.
    ///
    /// If the URL is an HTTP endpoint (e.g. `http://localhost:9222`),
    /// we first discover the WebSocket debugger URL via `/json`.
    pub async fn connect(url: &str, timeout: Duration) -> Result<Self> {
        let ws_url = if url.starts_with("ws://") || url.starts_with("wss://") {
            url.to_string()
        } else {
            discover_ws_url(url)
                .await
                .context("Failed to discover WebSocket URL from HTTP endpoint")?
        };

        debug!(ws_url = %ws_url, "Connecting to CDP target");

        let (ws_stream, _) = tokio::time::timeout(timeout, connect_async(&ws_url))
            .await
            .context("CDP WebSocket connection timed out")?
            .context("CDP WebSocket connection failed")?;

        let (sink, stream) = ws_stream.split();

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let events: EventMap = Arc::new(Mutex::new(HashMap::new()));
        let connected = Arc::new(Mutex::new(true));

        let reader_handle =
            spawn_reader(stream, pending.clone(), events.clone(), connected.clone());

        Ok(Self {
            ws_tx: Arc::new(Mutex::new(sink)),
            next_id: Arc::new(Mutex::new(1)),
            pending,
            events,
            reader_handle: Some(reader_handle),
            connected,
        })
    }

    /// Send a CDP method call and await the response.
    pub async fn send(&self, method: &str, params: Value) -> Result<Value> {
        if !*self.connected.lock().await {
            bail!("CDP client is disconnected");
        }

        let id = {
            let mut next = self.next_id.lock().await;
            let id = *next;
            *next += 1;
            id
        };

        let msg = json!({
            "id": id,
            "method": method,
            "params": params,
        });

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        {
            let mut sink = self.ws_tx.lock().await;
            sink.send(WsMessage::Text(msg.to_string().into()))
                .await
                .context("Failed to send CDP message")?;
        }

        tokio::time::timeout(SEND_TIMEOUT, rx)
            .await
            .context("CDP request timed out")??
    }

    /// Subscribe to a CDP event stream.
    pub async fn on(&self, event: &str) -> mpsc::UnboundedReceiver<Value> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut map = self.events.lock().await;
        map.entry(event.to_string()).or_default().push(tx);
        rx
    }

    /// Wait for a single occurrence of an event with a timeout.
    pub async fn wait_for_event(&self, event: &str, timeout: Duration) -> Result<Value> {
        let mut rx = self.on(event).await;
        tokio::time::timeout(timeout, rx.recv())
            .await
            .context(format!("Timed out waiting for event: {event}"))?
            .context(format!("Event channel closed for: {event}"))
    }

    /// Check if the client is still connected.
    pub async fn is_connected(&self) -> bool {
        *self.connected.lock().await
    }

    /// Gracefully disconnect from the CDP target.
    pub async fn disconnect(&self) {
        *self.connected.lock().await = false;

        // Close the WebSocket
        if let Ok(mut sink) = self.ws_tx.try_lock() {
            let _ = sink.close().await;
        }

        // Reject all pending requests
        let mut pending = self.pending.lock().await;
        for (_, tx) in pending.drain() {
            let _ = tx.send(Err(anyhow::anyhow!("CDP client disconnected")));
        }
    }
}

impl Drop for CdpClient {
    fn drop(&mut self) {
        if let Some(handle) = self.reader_handle.take() {
            handle.abort();
        }
    }
}

/// Spawn a task that reads WS messages and dispatches them.
fn spawn_reader(
    mut stream: WsStream,
    pending: PendingMap,
    events: EventMap,
    connected: Arc<Mutex<bool>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(msg_result) = stream.next().await {
            let msg = match msg_result {
                Ok(WsMessage::Text(text)) => text,
                Ok(WsMessage::Close(_)) => {
                    debug!("CDP WebSocket closed by server");
                    break;
                }
                Ok(_) => continue, // Ignore binary/ping/pong
                Err(e) => {
                    warn!("CDP WebSocket read error: {e}");
                    break;
                }
            };

            let value: Value = match serde_json::from_str(&msg) {
                Ok(v) => v,
                Err(e) => {
                    warn!("CDP: invalid JSON: {e}");
                    continue;
                }
            };

            // Response to a request (has "id")
            if let Some(id) = value.get("id").and_then(|v| v.as_u64()) {
                let mut map = pending.lock().await;
                if let Some(tx) = map.remove(&id) {
                    if let Some(error) = value.get("error") {
                        let msg = error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown CDP error");
                        let _ = tx.send(Err(anyhow::anyhow!("CDP error: {msg}")));
                    } else {
                        let result = value.get("result").cloned().unwrap_or(Value::Null);
                        let _ = tx.send(Ok(result));
                    }
                }
            }
            // Event (has "method" but no "id")
            else if let Some(method) = value.get("method").and_then(|v| v.as_str()) {
                let params = value.get("params").cloned().unwrap_or(Value::Null);
                let mut map = events.lock().await;
                if let Some(senders) = map.get_mut(method) {
                    // Remove closed channels
                    senders.retain(|tx| tx.send(params.clone()).is_ok());
                }
            }
        }

        *connected.lock().await = false;

        // Reject remaining pending requests
        let mut map = pending.lock().await;
        for (_, tx) in map.drain() {
            let _ = tx.send(Err(anyhow::anyhow!("CDP WebSocket connection lost")));
        }

        error!("CDP reader task exited");
    })
}

/// Discover the WebSocket debugger URL from an HTTP debugging endpoint.
///
/// Calls `GET /json` and finds the first page target.
/// Fallback: `PUT /json/new?about:blank` to create a new page.
pub async fn discover_ws_url(http_url: &str) -> Result<String> {
    let base = http_url.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    // Try /json to find an existing page target
    let resp = client.get(format!("{base}/json")).send().await?;
    let targets: Vec<Value> = resp.json().await?;

    for target in &targets {
        if target.get("type").and_then(|t| t.as_str()) == Some("page")
            && let Some(ws_url) = target.get("webSocketDebuggerUrl").and_then(|u| u.as_str())
        {
            return Ok(ws_url.to_string());
        }
    }

    // Fallback: create a new blank tab
    debug!("No existing page target found, creating new tab");
    let resp = client
        .put(format!("{base}/json/new?about:blank"))
        .send()
        .await?;
    let target: Value = resp.json().await?;
    target
        .get("webSocketDebuggerUrl")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .context("No webSocketDebuggerUrl in new tab response")
}
