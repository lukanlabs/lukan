use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use futures::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tracing::{error, info, warn};

use lukan_core::crypto::{self, E2EEnvelope, E2ESession};
use lukan_core::relay::{DaemonToRelay, RelayConfig, RelayToDaemon};

/// Relay bridge: connects the local daemon to the cloud relay server.
///
/// The bridge:
/// 1. Connects outbound to `wss://relay/ws/daemon` with `Authorization: Bearer` header
/// 2. Sends a `Register` message
/// 3. Forwards `RelayToDaemon::Forward` → local WebSocket (per connection)
/// 4. Forwards `RelayToDaemon::RestRequest` → local HTTP
/// 5. Handles E2E encryption handshake and message wrapping
/// 6. Auto-reconnects with exponential backoff
pub struct RelayBridge {
    relay_config: RelayConfig,
    local_port: u16,
    /// Channel to signal shutdown
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl RelayBridge {
    pub fn new(relay_config: RelayConfig, local_port: u16) -> Self {
        Self {
            relay_config,
            local_port,
            shutdown_tx: None,
        }
    }

    /// Start the relay bridge in a background task. Returns immediately.
    pub fn start(&mut self) {
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        let config = self.relay_config.clone();
        let port = self.local_port;

        tokio::spawn(async move {
            run_bridge_loop(config, port, shutdown_rx).await;
        });
    }

    /// Stop the relay bridge.
    pub fn stop(&mut self) {
        self.shutdown_tx.take();
    }
}

/// Main bridge loop with auto-reconnect.
async fn run_bridge_loop(
    config: RelayConfig,
    local_port: u16,
    mut shutdown_rx: mpsc::Receiver<()>,
) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        info!(relay_url = %config.relay_url, "Connecting to relay server...");

        let started = std::time::Instant::now();
        match connect_and_run(&config, local_port, &mut shutdown_rx).await {
            Ok(()) => {
                info!("Relay bridge shut down gracefully");
                return;
            }
            Err(e) => {
                warn!(error = %e, cause = ?e, backoff_secs = backoff.as_secs(), "Relay connection failed, retrying");
            }
        }

        // If we were connected for >10s, reset backoff (likely a server restart, not a permanent failure)
        if started.elapsed() > Duration::from_secs(10) {
            backoff = Duration::from_secs(1);
        }

        // Wait with backoff, but allow shutdown during the wait
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            _ = shutdown_rx.recv() => {
                info!("Relay bridge shutdown requested during backoff");
                return;
            }
        }

        backoff = (backoff * 2).min(max_backoff);
    }
}

/// Single connection attempt: connect, authenticate, and process messages.
async fn connect_and_run(
    config: &RelayConfig,
    local_port: u16,
    shutdown_rx: &mut mpsc::Receiver<()>,
) -> Result<()> {
    // Build the WebSocket URL (convert http(s) to ws(s) if needed)
    let base = config
        .relay_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    let ws_url = format!("{}/ws/daemon", base.trim_end_matches('/'));

    // Send JWT via Authorization header instead of query string
    // to prevent token leakage in server logs, proxy logs, and browser history
    let request = tungstenite::http::Request::builder()
        .uri(&ws_url)
        .header("authorization", format!("Bearer {}", config.jwt_token))
        .header(
            "sec-websocket-key",
            tungstenite::handshake::client::generate_key(),
        )
        .header("sec-websocket-version", "13")
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header(
            "host",
            tungstenite::http::Uri::try_from(&ws_url)
                .ok()
                .and_then(|u| u.host().map(|h| h.to_string()))
                .unwrap_or_else(|| "localhost".to_string()),
        )
        .body(())
        .context("Failed to build WebSocket request")?;

    let ws_config = tungstenite::protocol::WebSocketConfig::default()
        .max_frame_size(Some(64 * 1024 * 1024))
        .max_message_size(Some(64 * 1024 * 1024));
    let (ws_stream, _) =
        tokio_tungstenite::connect_async_with_config(request, Some(ws_config), false)
            .await
            .context("Failed to connect to relay")?;

    info!("Connected to relay server");

    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    // Send Register message with system info
    let register = serde_json::to_string(&DaemonToRelay::Register {
        user_id: config.user_id.clone(),
        device_name: hostname(),
        os: Some(format!(
            "{} {}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
    })?;
    ws_tx
        .send(tungstenite::Message::Text(register.into()))
        .await?;

    // Track local WebSocket connections to the web server (connection_id → sender)
    let local_connections: Arc<tokio::sync::Mutex<HashMap<String, LocalWsConnection>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    // E2E state per connection — created on ConnectionOpened, shared with local WS
    let e2e_states: Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<E2EState>>>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    // Channel for daemon → relay messages (responses from local processing)
    let (response_tx, mut response_rx) = mpsc::unbounded_channel::<String>();

    // Spawn a dedicated writer task so large messages don't block the main
    // select! loop (which must keep processing incoming relay messages and pings).
    // Use a bounded channel for write requests so backpressure is applied
    // to per-connection tasks rather than stalling the reader.
    let (write_tx, mut write_rx) = mpsc::channel::<tungstenite::Message>(64);
    let writer_task = tokio::spawn(async move {
        while let Some(msg) = write_rx.recv().await {
            if ws_tx.send(msg).await.is_err() {
                break;
            }
        }
        let _ = ws_tx.close().await;
    });

    // Reset backoff on successful connection
    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
    ping_interval.tick().await; // skip first tick

    let result = loop {
        tokio::select! {
            // Incoming message from relay
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(tungstenite::Message::Text(text))) => {
                        handle_relay_message(
                            &text,
                            local_port,
                            &local_connections,
                            &e2e_states,
                            &response_tx,
                        ).await;
                    }
                    Some(Ok(tungstenite::Message::Close(_))) | None => {
                        info!("Relay connection closed");
                        break Err(anyhow::anyhow!("Connection closed"));
                    }
                    Some(Err(e)) => {
                        break Err(anyhow::anyhow!("WebSocket error: {e}"));
                    }
                    _ => {}
                }
            }

            // Outgoing message to relay (forwarded responses)
            Some(msg) = response_rx.recv() => {
                if write_tx.send(tungstenite::Message::Text(msg.into())).await.is_err() {
                    break Err(anyhow::anyhow!("Failed to send to relay"));
                }
            }

            // Periodic ping
            _ = ping_interval.tick() => {
                let ping = serde_json::to_string(&DaemonToRelay::Ping).unwrap();
                if write_tx.send(tungstenite::Message::Text(ping.into())).await.is_err() {
                    break Err(anyhow::anyhow!("Ping failed"));
                }
            }

            // Shutdown signal
            _ = shutdown_rx.recv() => {
                drop(write_tx);
                let _ = writer_task.await;
                return Ok(());
            }
        }
    };

    // Cleanup: close all local WS connections so the daemon's ws_handler
    // runs its disconnect handler (which saves sessions). Without this,
    // forwarding tasks hang indefinitely after a relay disconnect — their
    // rx.recv() never returns None because the Arc<HashMap> keeps tx alive.
    {
        let mut conns = local_connections.lock().await;
        for (conn_id, conn) in conns.drain() {
            info!(connection_id = %conn_id, "Closing local WS on relay disconnect");
            conn.task.abort();
        }
    }

    drop(write_tx);
    writer_task.abort();
    result
}

// ── E2E encryption state machine ──────────────────────────────

/// Per-connection E2E encryption state.
enum E2EState {
    /// Waiting for browser's e2e_hello. Buffer messages from local WS until handshake completes.
    AwaitingHello { queued: Vec<String> },
    /// Handshake complete — encrypt outgoing, decrypt incoming.
    Established(Box<E2ESession>),
    /// No encryption (old browser or handshake timeout). Pass through plaintext.
    Passthrough,
}

/// A local WebSocket connection to the web server, representing one browser client session.
struct LocalWsConnection {
    tx: mpsc::UnboundedSender<String>,
    #[allow(dead_code)]
    e2e: Arc<tokio::sync::Mutex<E2EState>>,
    task: tokio::task::JoinHandle<()>,
}

type E2EStates = Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<E2EState>>>>>;

/// Process a single message from the relay.
async fn handle_relay_message(
    text: &str,
    local_port: u16,
    local_connections: &Arc<tokio::sync::Mutex<HashMap<String, LocalWsConnection>>>,
    e2e_states: &E2EStates,
    response_tx: &mpsc::UnboundedSender<String>,
) {
    let msg: RelayToDaemon = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, "Invalid relay message");
            return;
        }
    };

    match msg {
        RelayToDaemon::Forward {
            connection_id,
            message,
        } => {
            handle_forward(
                &connection_id,
                message,
                local_connections,
                e2e_states,
                response_tx,
            )
            .await;
        }
        RelayToDaemon::RestRequest {
            request_id,
            method,
            path,
            headers,
            body,
            target_port,
        } => {
            // Check if this is an E2E REST tunnel request
            if path == "/api/_e2e" {
                handle_e2e_rest(
                    request_id,
                    &body,
                    local_port,
                    local_connections,
                    e2e_states,
                    response_tx,
                )
                .await;
            } else {
                // Use target_port if specified (port tunnel), otherwise daemon's own port
                let port = target_port.unwrap_or(local_port);
                let response_tx = response_tx.clone();
                tokio::spawn(async move {
                    let result = tunnel_rest_request(port, &method, &path, &headers, &body).await;
                    let resp = match result {
                        Ok((status, resp_headers, resp_body)) => DaemonToRelay::RestResponse {
                            request_id,
                            status,
                            headers: resp_headers,
                            body: resp_body,
                        },
                        Err(e) => {
                            error!(error = %e, "Local REST request failed");
                            DaemonToRelay::RestResponse {
                                request_id,
                                status: 502,
                                headers: HashMap::new(),
                                body: format!("Local server error: {e}").into_bytes(),
                            }
                        }
                    };
                    let json = serde_json::to_string(&resp).unwrap();
                    let _ = response_tx.send(json);
                });
            }
        }
        RelayToDaemon::ConnectionOpened { connection_id } => {
            // Pre-create E2E state so handshake can happen before local WS connects
            let e2e = Arc::new(tokio::sync::Mutex::new(E2EState::AwaitingHello {
                queued: Vec::new(),
            }));
            {
                let mut states = e2e_states.lock().await;
                states.insert(connection_id.clone(), Arc::clone(&e2e));
            }

            // Open a local WebSocket to the web server for this browser connection
            let conns = Arc::clone(local_connections);
            let response_tx = response_tx.clone();
            let port = local_port;
            let e2e_for_conn = Arc::clone(&e2e);
            let e2e_states_clone = Arc::clone(e2e_states);
            tokio::spawn(async move {
                if let Err(e) = open_local_ws_connection(
                    &connection_id,
                    port,
                    &conns,
                    &response_tx,
                    e2e_for_conn,
                    &e2e_states_clone,
                )
                .await
                {
                    error!(
                        connection_id = %connection_id,
                        error = %e,
                        "Failed to open local WS connection"
                    );
                }
            });
        }
        RelayToDaemon::ConnectionClosed { connection_id } => {
            // Close the local WebSocket for this browser connection and abort its forwarding task
            let mut conns = local_connections.lock().await;
            if let Some(conn) = conns.remove(&connection_id) {
                conn.task.abort();
                info!(connection_id = %connection_id, "Closed local WS connection");
            }
            // Clean up E2E state
            let mut states = e2e_states.lock().await;
            states.remove(&connection_id);
        }
        RelayToDaemon::Pong => {
            // Heartbeat response, nothing to do
        }
    }
}

/// Handle a Forward message — may be plaintext, E2E handshake, or encrypted.
async fn handle_forward(
    connection_id: &str,
    message: serde_json::Value,
    local_connections: &Arc<tokio::sync::Mutex<HashMap<String, LocalWsConnection>>>,
    e2e_states: &E2EStates,
    response_tx: &mpsc::UnboundedSender<String>,
) {
    let msg_type = message.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "e2e_hello" => {
            // Browser is initiating E2E handshake
            let browser_pk_b64 = match message.get("pk").and_then(|v| v.as_str()) {
                Some(pk) => pk.to_string(),
                None => {
                    warn!(connection_id = %connection_id, "e2e_hello missing pk");
                    return;
                }
            };

            let browser_pk_bytes: [u8; 32] = match B64.decode(&browser_pk_b64) {
                Ok(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    arr
                }
                _ => {
                    warn!(connection_id = %connection_id, "e2e_hello: invalid pk");
                    return;
                }
            };

            // Generate our keypair and derive shared secret
            let (secret, our_pk) = crypto::generate_keypair();
            let shared = crypto::ecdh(secret, &browser_pk_bytes);
            let session = E2ESession::from_shared_secret(&shared, &our_pk, &browser_pk_bytes);

            let safety_number = session.safety_number.clone();
            info!(
                connection_id = %connection_id,
                safety_number = %safety_number,
                "E2E handshake complete"
            );

            // Send e2e_hello_ack back to browser via relay
            let ack = E2EEnvelope::E2eHelloAck {
                pk: B64.encode(our_pk),
                safety_number: safety_number.clone(),
            };
            let forward = serde_json::to_string(&DaemonToRelay::Forward {
                connection_id: connection_id.to_string(),
                message: serde_json::to_value(&ack).unwrap(),
            })
            .unwrap();
            let _ = response_tx.send(forward);

            // Transition state to Established and flush queued messages
            let states = e2e_states.lock().await;
            if let Some(e2e) = states.get(connection_id) {
                let mut state = e2e.lock().await;
                let queued = match &mut *state {
                    E2EState::AwaitingHello { queued } => std::mem::take(queued),
                    _ => vec![],
                };
                *state = E2EState::Established(Box::new(session));

                // Now encrypt and send any queued outgoing messages
                if !queued.is_empty()
                    && let E2EState::Established(ref mut sess) = *state
                {
                    for msg in queued {
                        let envelope = sess.encrypt(msg.as_bytes());
                        let fwd = serde_json::to_string(&DaemonToRelay::Forward {
                            connection_id: connection_id.to_string(),
                            message: serde_json::to_value(&envelope).unwrap(),
                        })
                        .unwrap();
                        let _ = response_tx.send(fwd);
                    }
                }
            }
        }

        "e2e" => {
            // Encrypted message from browser — decrypt and forward to local WS
            let nonce = match message.get("n").and_then(|v| v.as_str()) {
                Some(n) => n,
                None => {
                    warn!(connection_id = %connection_id, "e2e message missing nonce");
                    return;
                }
            };
            let ciphertext = match message.get("d").and_then(|v| v.as_str()) {
                Some(d) => d,
                None => {
                    warn!(connection_id = %connection_id, "e2e message missing data");
                    return;
                }
            };

            let states = e2e_states.lock().await;
            if let Some(e2e) = states.get(connection_id) {
                let state = e2e.lock().await;
                match &*state {
                    E2EState::Established(session) => match session.decrypt(nonce, ciphertext) {
                        Ok(plaintext) => {
                            let plaintext_str = String::from_utf8(plaintext).unwrap_or_default();
                            // Forward to local WS if connected
                            drop(state);
                            drop(states);
                            let conns = local_connections.lock().await;
                            if let Some(conn) = conns.get(connection_id)
                                && conn.tx.send(plaintext_str).is_err()
                            {
                                warn!(connection_id = %connection_id, "Local WS connection dead");
                            }
                        }
                        Err(e) => {
                            warn!(connection_id = %connection_id, error = %e, "E2E decrypt failed");
                        }
                    },
                    _ => {
                        warn!(connection_id = %connection_id, "Got e2e message but session not established");
                    }
                }
            }
        }

        _ => {
            // Regular plaintext message — forward as-is if Passthrough or not yet E2E
            let conns = local_connections.lock().await;
            if let Some(conn) = conns.get(connection_id) {
                let json = serde_json::to_string(&message).unwrap_or_default();
                if conn.tx.send(json).is_err() {
                    warn!(connection_id = %connection_id, "Local WS connection dead");
                }
            } else {
                drop(conns);
                warn!(connection_id = %connection_id, "No local WS connection, message dropped (connection may not be opened yet)");
            }
        }
    }
}

/// Handle an E2E REST tunnel request: decrypt, execute locally, encrypt response.
async fn handle_e2e_rest(
    request_id: String,
    body: &[u8],
    local_port: u16,
    _local_connections: &Arc<tokio::sync::Mutex<HashMap<String, LocalWsConnection>>>,
    e2e_states: &E2EStates,
    response_tx: &mpsc::UnboundedSender<String>,
) {
    // Parse the E2E REST envelope: { connection_id, n, d }
    let envelope: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "Invalid E2E REST body");
            send_rest_response(response_tx, &request_id, 400, b"Invalid E2E REST body");
            return;
        }
    };

    let connection_id = match envelope.get("connection_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => {
            send_rest_response(response_tx, &request_id, 400, b"Missing connection_id");
            return;
        }
    };

    let nonce = match envelope.get("n").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            send_rest_response(response_tx, &request_id, 400, b"Missing nonce");
            return;
        }
    };

    let ciphertext = match envelope.get("d").and_then(|v| v.as_str()) {
        Some(d) => d.to_string(),
        None => {
            send_rest_response(response_tx, &request_id, 400, b"Missing data");
            return;
        }
    };

    // Find the E2E session for this connection
    let states = e2e_states.lock().await;
    let e2e = match states.get(&connection_id) {
        Some(e) => Arc::clone(e),
        None => {
            send_rest_response(response_tx, &request_id, 400, b"Unknown connection_id");
            return;
        }
    };
    drop(states);

    // Decrypt the request
    let plaintext = {
        let state = e2e.lock().await;
        match &*state {
            E2EState::Established(session) => match session.decrypt(&nonce, &ciphertext) {
                Ok(pt) => pt,
                Err(e) => {
                    warn!(error = %e, "E2E REST decrypt failed");
                    send_rest_response(response_tx, &request_id, 400, b"Decrypt failed");
                    return;
                }
            },
            _ => {
                send_rest_response(response_tx, &request_id, 400, b"E2E not established");
                return;
            }
        }
    };

    // Parse decrypted request: { method, path, body? }
    let inner: serde_json::Value = match serde_json::from_slice(&plaintext) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "Invalid decrypted REST request");
            send_rest_response(response_tx, &request_id, 400, b"Invalid inner request");
            return;
        }
    };

    let method = inner
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET");
    let path = inner.get("path").and_then(|v| v.as_str()).unwrap_or("/");
    let inner_body = inner
        .get("body")
        .map(|v| v.to_string().into_bytes())
        .unwrap_or_default();
    let inner_headers: HashMap<String, String> = inner
        .get("headers")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Execute the real REST request locally
    let result = tunnel_rest_request(local_port, method, path, &inner_headers, &inner_body).await;

    let (status, resp_headers, resp_body) = match result {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "Local E2E REST request failed");
            (502, HashMap::new(), format!("Error: {e}").into_bytes())
        }
    };

    // Build the plaintext response
    let resp_json = serde_json::json!({
        "status": status,
        "headers": resp_headers,
        "body": B64.encode(&resp_body),
    });
    let resp_bytes = serde_json::to_vec(&resp_json).unwrap();

    // Encrypt the response
    let mut state = e2e.lock().await;
    let encrypted_body = match &mut *state {
        E2EState::Established(session) => {
            let envelope = session.encrypt(&resp_bytes);
            serde_json::to_vec(&envelope).unwrap()
        }
        _ => {
            send_rest_response(response_tx, &request_id, 500, b"E2E session lost");
            return;
        }
    };
    drop(state);

    // Send encrypted response back
    let resp = DaemonToRelay::RestResponse {
        request_id,
        status: 200,
        headers: {
            let mut h = HashMap::new();
            h.insert("content-type".to_string(), "application/json".to_string());
            h
        },
        body: encrypted_body,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let _ = response_tx.send(json);
}

fn send_rest_response(
    response_tx: &mpsc::UnboundedSender<String>,
    request_id: &str,
    status: u16,
    body: &[u8],
) {
    let resp = DaemonToRelay::RestResponse {
        request_id: request_id.to_string(),
        status,
        headers: HashMap::new(),
        body: body.to_vec(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    let _ = response_tx.send(json);
}

/// Open a local WebSocket connection to the web server for a given browser session.
async fn open_local_ws_connection(
    connection_id: &str,
    local_port: u16,
    connections: &Arc<tokio::sync::Mutex<HashMap<String, LocalWsConnection>>>,
    response_tx: &mpsc::UnboundedSender<String>,
    e2e: Arc<tokio::sync::Mutex<E2EState>>,
    _e2e_states: &E2EStates,
) -> Result<()> {
    let ws_url = format!("ws://127.0.0.1:{local_port}/ws");
    // Add x-relay-internal header so daemon skips web_password auth
    let request = tungstenite::http::Request::builder()
        .uri(&ws_url)
        .header("x-relay-internal", "true")
        .header(
            "sec-websocket-key",
            tungstenite::handshake::client::generate_key(),
        )
        .header("sec-websocket-version", "13")
        .header("connection", "Upgrade")
        .header("upgrade", "websocket")
        .header("host", format!("127.0.0.1:{local_port}"))
        .body(())
        .context("Failed to build WS request")?;
    let (ws_stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .context("Failed to connect to local web server")?;

    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Spawn a 5s timeout: if no e2e_hello received, fall back to Passthrough
    let e2e_timeout = Arc::clone(&e2e);
    let timeout_conn_id = connection_id.to_string();
    let timeout_response_tx = response_tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let mut state = e2e_timeout.lock().await;
        if let E2EState::AwaitingHello { queued } = &mut *state {
            let queued_msgs = std::mem::take(queued);
            info!(connection_id = %timeout_conn_id, "E2E handshake timeout, falling back to passthrough");
            *state = E2EState::Passthrough;

            // Flush queued messages as plaintext
            for msg in queued_msgs {
                let forward = serde_json::to_string(&DaemonToRelay::Forward {
                    connection_id: timeout_conn_id.clone(),
                    message: serde_json::from_str::<serde_json::Value>(&msg)
                        .unwrap_or(serde_json::Value::Null),
                })
                .unwrap();
                let _ = timeout_response_tx.send(forward);
            }
        }
    });

    let conn_id = connection_id.to_string();
    let response_tx = response_tx.clone();
    let connections_clone = Arc::clone(connections);
    let e2e_for_insert = Arc::clone(&e2e);

    // Spawn a task to handle bidirectional message forwarding
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Messages from the browser (via relay) → local web server
                Some(msg) = rx.recv() => {
                    if ws_tx.send(tungstenite::Message::Text(msg.into())).await.is_err() {
                        break;
                    }
                }
                // Messages from local web server → relay → browser
                msg = ws_rx.next() => {
                    match msg {
                        Some(Ok(tungstenite::Message::Text(text))) => {
                            let mut state = e2e.lock().await;
                            let outgoing = match &mut *state {
                                E2EState::Established(session) => {
                                    // Encrypt before sending
                                    let envelope = session.encrypt(text.as_bytes());
                                    serde_json::to_value(&envelope).unwrap()
                                }
                                E2EState::AwaitingHello { queued } => {
                                    // Buffer until handshake completes
                                    queued.push(text.to_string());
                                    continue;
                                }
                                E2EState::Passthrough => {
                                    // Send plaintext
                                    serde_json::from_str::<serde_json::Value>(&text)
                                        .unwrap_or(serde_json::Value::Null)
                                }
                            };
                            drop(state);

                            let forward = serde_json::to_string(&DaemonToRelay::Forward {
                                connection_id: conn_id.clone(),
                                message: outgoing,
                            }).unwrap();
                            if response_tx.send(forward).is_err() {
                                break;
                            }
                        }
                        Some(Ok(tungstenite::Message::Close(_))) | None => break,
                        _ => {}
                    }
                }
            }
        }

        // Cleanup
        let mut conns = connections_clone.lock().await;
        conns.remove(&conn_id);
    });

    // Register the connection with its task handle so it can be aborted on ConnectionClosed
    {
        let mut conns = connections.lock().await;
        conns.insert(
            connection_id.to_string(),
            LocalWsConnection {
                tx,
                e2e: e2e_for_insert,
                task,
            },
        );
    }

    info!(connection_id = %connection_id, "Local WS connection opened");
    Ok(())
}

/// Make a local HTTP request to the web server and return the response.
async fn tunnel_rest_request(
    local_port: u16,
    method: &str,
    path: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> Result<(u16, HashMap<String, String>, Vec<u8>)> {
    let url = format!("http://127.0.0.1:{local_port}{path}");
    let client = reqwest::Client::new();

    let mut builder = match method.to_uppercase().as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        "HEAD" => client.head(&url),
        _ => client.get(&url),
    };

    // Mark as relay-internal so daemon skips web_password auth
    builder = builder.header("x-relay-internal", "true");

    for (k, v) in headers {
        builder = builder.header(k.as_str(), v.as_str());
    }

    if !body.is_empty() {
        // Default to application/json if no content-type was provided
        if !headers
            .keys()
            .any(|k| k.eq_ignore_ascii_case("content-type"))
        {
            builder = builder.header("content-type", "application/json");
        }
        builder = builder.body(body.to_vec());
    }

    let resp = builder.send().await.context("Local HTTP request failed")?;
    let status = resp.status().as_u16();

    let mut resp_headers = HashMap::new();
    for (k, v) in resp.headers() {
        if let Ok(val) = v.to_str() {
            resp_headers.insert(k.as_str().to_string(), val.to_string());
        }
    }

    let resp_body = resp.bytes().await?.to_vec();

    Ok((status, resp_headers, resp_body))
}

/// Get the system hostname for device identification.
fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".into())
}
