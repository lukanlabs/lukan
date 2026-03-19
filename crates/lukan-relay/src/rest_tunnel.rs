use std::collections::HashMap;
use std::time::Duration;

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tracing::warn;

use lukan_core::relay::RelayToDaemon;

use crate::auth;
use crate::state::{PendingRestRequest, SharedState};

/// Timeout for REST tunnel requests (seconds).
const TUNNEL_TIMEOUT_SECS: u64 = 60;

/// Extract device name from request header.
fn extract_device(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-lukan-device")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

/// E2E encrypted REST tunnel: passes encrypted blobs to/from the daemon.
/// The relay cannot decrypt the content — it's just a pass-through.
pub async fn e2e_rest_tunnel_handler(
    State(state): State<SharedState>,
    request: Request<Body>,
) -> Response {
    let token = auth::extract_token_from_cookie(request.headers()).or_else(|| {
        request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string())
    });

    let device = match extract_device(request.headers()) {
        Some(d) => d,
        None => return (StatusCode::BAD_REQUEST, "Missing x-lukan-device header").into_response(),
    };

    let claims = match token {
        Some(t) => auth::verify_jwt(&state.browser_jwt_secret(), &t)
            .or_else(|_| auth::verify_jwt(&state.jwt_secret, &t))
            .map_err(|_| ()),
        None => Err(()),
    };

    let claims = match claims {
        Ok(c) => c,
        Err(()) => {
            return (StatusCode::UNAUTHORIZED, "Missing or invalid authorization").into_response();
        }
    };

    if !state.has_daemon(&claims.sub, &device) {
        return (StatusCode::BAD_GATEWAY, "Daemon not connected").into_response();
    }

    // Read the encrypted body (pass through as-is)
    let body_bytes = match axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b.to_vec(),
        Err(_) => return (StatusCode::BAD_REQUEST, "Body too large").into_response(),
    };

    let request_id = uuid::Uuid::new_v4().to_string();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    state
        .pending_rest
        .insert(request_id.clone(), PendingRestRequest { tx: resp_tx });

    // Send to daemon as a RestRequest with path "/_e2e" — daemon knows to handle it
    let relay_msg = serde_json::to_string(&RelayToDaemon::RestRequest {
        request_id: request_id.clone(),
        method: "POST".to_string(),
        path: "/api/_e2e".to_string(),
        headers: HashMap::new(),
        body: body_bytes,
        target_port: None,
    })
    .unwrap();

    if !state.send_to_daemon(&claims.sub, &device, &relay_msg) {
        state.pending_rest.remove(&request_id);
        return (StatusCode::BAD_GATEWAY, "Failed to send to daemon").into_response();
    }

    // Wait for daemon response
    match tokio::time::timeout(Duration::from_secs(TUNNEL_TIMEOUT_SECS), resp_rx).await {
        Ok(Ok(tunnel_resp)) => {
            let status =
                StatusCode::from_u16(tunnel_resp.status).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut response = Response::builder().status(status);
            for (k, v) in &tunnel_resp.headers {
                response = response.header(k.as_str(), v.as_str());
            }
            response
                .body(Body::from(tunnel_resp.body))
                .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "").into_response())
        }
        Ok(Err(_)) => {
            state.pending_rest.remove(&request_id);
            (StatusCode::BAD_GATEWAY, "Daemon disconnected").into_response()
        }
        Err(_) => {
            state.pending_rest.remove(&request_id);
            (StatusCode::GATEWAY_TIMEOUT, "E2E request timed out").into_response()
        }
    }
}

/// Catch-all handler for `/api/*` routes — tunnels requests to the user's daemon.
pub async fn rest_tunnel_handler(
    State(state): State<SharedState>,
    request: Request<Body>,
) -> Response {
    let token = auth::extract_token_from_cookie(request.headers()).or_else(|| {
        request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string())
    });

    let device = match extract_device(request.headers()) {
        Some(d) => d,
        None => return (StatusCode::BAD_REQUEST, "Missing x-lukan-device header").into_response(),
    };

    let claims = match token {
        Some(t) => auth::verify_jwt(&state.browser_jwt_secret(), &t)
            .or_else(|_| auth::verify_jwt(&state.jwt_secret, &t))
            .map_err(|e| {
                let path = request.uri().path();
                tracing::warn!(path = %path, error = %e, "REST tunnel auth failed");
            }),
        None => {
            let path = request.uri().path();
            tracing::warn!(path = %path, "REST tunnel: no token found in cookie or header");
            Err(())
        }
    };

    let claims = match claims {
        Ok(c) => c,
        Err(()) => {
            return (StatusCode::UNAUTHORIZED, "Missing or invalid authorization").into_response();
        }
    };

    if !state.has_daemon(&claims.sub, &device) {
        return (
            StatusCode::BAD_GATEWAY,
            "Your local lukan daemon is not connected. Run `lukan daemon start` and `lukan login`.",
        )
            .into_response();
    }

    let method = request.method().to_string();
    let path = request.uri().path().to_string();

    // Collect headers
    let mut headers = HashMap::new();
    for (k, v) in request.headers() {
        if let Ok(val) = v.to_str() {
            // Skip hop-by-hop headers and auth (daemon doesn't need relay auth)
            let name = k.as_str().to_lowercase();
            if name != "authorization" && name != "host" && name != "connection" {
                headers.insert(name, val.to_string());
            }
        }
    }

    // Read body
    let body_bytes = match axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b.to_vec(),
        Err(_) => return (StatusCode::BAD_REQUEST, "Body too large").into_response(),
    };

    let request_id = uuid::Uuid::new_v4().to_string();

    // Create oneshot channel for the response
    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    state
        .pending_rest
        .insert(request_id.clone(), PendingRestRequest { tx: resp_tx });

    // Send the REST request to the daemon
    let relay_msg = serde_json::to_string(&RelayToDaemon::RestRequest {
        request_id: request_id.clone(),
        method,
        path: path.clone(),
        headers,
        body: body_bytes,
        target_port: None,
    })
    .unwrap();

    if !state.send_to_daemon(&claims.sub, &device, &relay_msg) {
        state.pending_rest.remove(&request_id);
        return (StatusCode::BAD_GATEWAY, "Failed to send to daemon").into_response();
    }

    // Wait for response with timeout
    match tokio::time::timeout(Duration::from_secs(TUNNEL_TIMEOUT_SECS), resp_rx).await {
        Ok(Ok(tunnel_resp)) => {
            let status =
                StatusCode::from_u16(tunnel_resp.status).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut response = Response::builder().status(status);

            for (k, v) in &tunnel_resp.headers {
                response = response.header(k.as_str(), v.as_str());
            }

            response
                .body(Body::from(tunnel_resp.body))
                .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "").into_response())
        }
        Ok(Err(_)) => {
            // Oneshot sender was dropped (daemon disconnected)
            state.pending_rest.remove(&request_id);
            (StatusCode::BAD_GATEWAY, "Daemon disconnected").into_response()
        }
        Err(_) => {
            // Timeout
            state.pending_rest.remove(&request_id);
            warn!(path = %path, "REST tunnel request timed out");
            (StatusCode::GATEWAY_TIMEOUT, "Request timed out").into_response()
        }
    }
}

/// Port tunnel handler: proxy requests to a specific port on the user's machine.
/// Supports two URL patterns:
///   /tunnel/{port}/...           — uses first connected device
///   /tunnel/{device}/{port}/...  — targets specific device
pub async fn port_tunnel_handler(
    State(state): State<SharedState>,
    Path(raw_path): Path<String>,
    request: Request<Body>,
) -> Response {
    // Parse: first segment is either a port (number) or device name
    let segments: Vec<&str> = raw_path.splitn(3, '/').collect();
    let (device, port, path) = match segments.as_slice() {
        [first, rest @ ..] => {
            if let Ok(p) = first.parse::<u16>() {
                // /tunnel/{port}/...
                (None, p, rest.join("/"))
            } else if let Some(second) = rest.first()
                && let Ok(p) = second.parse::<u16>()
            {
                // /tunnel/{device}/{port}/...
                let remaining = if rest.len() > 1 {
                    rest[1..].join("/")
                } else {
                    String::new()
                };
                (Some(first.to_string()), p, remaining)
            } else {
                return (
                    StatusCode::BAD_REQUEST,
                    "Invalid tunnel URL. Use /tunnel/{port}/path or /tunnel/{device}/{port}/path",
                )
                    .into_response();
            }
        }
        _ => return (StatusCode::BAD_REQUEST, "Invalid tunnel URL").into_response(),
    };
    port_tunnel_inner(state, device, port, path, request).await
}

async fn port_tunnel_inner(
    state: SharedState,
    explicit_device: Option<String>,
    port: u16,
    path: String,
    request: Request<Body>,
) -> Response {
    let token = auth::extract_token_from_cookie(request.headers()).or_else(|| {
        request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string())
    });

    let claims = match token {
        Some(t) => auth::verify_jwt(&state.browser_jwt_secret(), &t)
            .or_else(|_| auth::verify_jwt(&state.jwt_secret, &t))
            .map_err(|_| ()),
        None => Err(()),
    };

    let claims = match claims {
        Ok(c) => c,
        Err(()) => {
            return (StatusCode::UNAUTHORIZED, "Missing or invalid authorization").into_response();
        }
    };

    // Resolve device: explicit > header > first connected
    let device = explicit_device
        .or_else(|| extract_device(request.headers()))
        .unwrap_or_else(|| {
            state
                .list_device_names(&claims.sub)
                .into_iter()
                .next()
                .unwrap_or_default()
        });

    if device.is_empty() || !state.has_daemon(&claims.sub, &device) {
        return (StatusCode::BAD_GATEWAY, "Daemon not connected").into_response();
    }

    let method = request.method().to_string();
    let tunnel_path = format!("/{path}");

    let mut headers = HashMap::new();
    for (k, v) in request.headers() {
        if let Ok(val) = v.to_str() {
            let name = k.as_str().to_lowercase();
            if name != "authorization" && name != "host" && name != "connection" {
                headers.insert(name, val.to_string());
            }
        }
    }

    let body_bytes = match axum::body::to_bytes(request.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b.to_vec(),
        Err(_) => return (StatusCode::BAD_REQUEST, "Body too large").into_response(),
    };

    let request_id = uuid::Uuid::new_v4().to_string();

    let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
    state
        .pending_rest
        .insert(request_id.clone(), PendingRestRequest { tx: resp_tx });

    let relay_msg = serde_json::to_string(&RelayToDaemon::RestRequest {
        request_id: request_id.clone(),
        method,
        path: tunnel_path.clone(),
        headers,
        body: body_bytes,
        target_port: Some(port),
    })
    .unwrap();

    if !state.send_to_daemon(&claims.sub, &device, &relay_msg) {
        state.pending_rest.remove(&request_id);
        return (StatusCode::BAD_GATEWAY, "Failed to send to daemon").into_response();
    }

    match tokio::time::timeout(Duration::from_secs(TUNNEL_TIMEOUT_SECS), resp_rx).await {
        Ok(Ok(tunnel_resp)) => {
            let status =
                StatusCode::from_u16(tunnel_resp.status).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut response = Response::builder().status(status);
            for (k, v) in &tunnel_resp.headers {
                response = response.header(k.as_str(), v.as_str());
            }
            response
                .body(Body::from(tunnel_resp.body))
                .unwrap_or_else(|_| (StatusCode::INTERNAL_SERVER_ERROR, "").into_response())
        }
        Ok(Err(_)) => {
            state.pending_rest.remove(&request_id);
            (StatusCode::BAD_GATEWAY, "Daemon disconnected").into_response()
        }
        Err(_) => {
            state.pending_rest.remove(&request_id);
            warn!(port, path = %tunnel_path, "Port tunnel request timed out");
            (StatusCode::GATEWAY_TIMEOUT, "Request timed out").into_response()
        }
    }
}
