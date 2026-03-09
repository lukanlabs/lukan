#![allow(dead_code)]

mod auth;
mod rest_tunnel;
mod state;
mod static_files;
mod ws_client;
mod ws_daemon;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use crate::state::{RelayState, SharedState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "lukan_relay=info".parse().unwrap()),
        )
        .init();

    let state = Arc::new(RelayState::new());

    // Dev mode warnings
    if state.dev_mode {
        warn!("========================================================");
        warn!("  WARNING: DEV MODE is enabled!");
        warn!("  /auth/dev endpoints are active — do NOT use in production");
        warn!("========================================================");
        if state.dev_secret.is_none() {
            warn!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
            warn!("  CRITICAL: No RELAY_DEV_SECRET set!");
            warn!("  Anyone can obtain tokens without authentication.");
            warn!("  Set RELAY_DEV_SECRET to require a secret for dev login.");
            warn!("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!");
        }
    }
    info!(
        "Boot ID: {} (browser tokens from previous boots are now invalid)",
        &state.boot_id[..8]
    );

    let router = create_router(state.clone());

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let addr = format!("0.0.0.0:{port}");
    info!("lukan-relay listening on {addr}");
    info!("Public URL: {}", state.public_url);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}

fn create_router(state: SharedState) -> Router {
    // CORS: allow the relay's own origin + any additional origins from RELAY_CORS_ORIGINS env var.
    // This lets external UIs (self-hosted dashboards, etc.) call the relay API with credentials.
    let mut origins: Vec<axum::http::HeaderValue> = vec![state
        .public_url
        .parse::<axum::http::HeaderValue>()
        .unwrap()];
    if let Ok(extra) = std::env::var("RELAY_CORS_ORIGINS") {
        for origin in extra.split(',') {
            let origin = origin.trim();
            if !origin.is_empty() {
                if let Ok(val) = origin.parse::<axum::http::HeaderValue>() {
                    origins.push(val);
                }
            }
        }
    }
    let cors = CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
            axum::http::header::COOKIE,
        ])
        .allow_credentials(true);

    Router::new()
        // Auth
        .route("/auth/google", get(auth::google_login))
        .route("/auth/callback", get(auth::google_callback))
        .route("/auth/status", get(auth::auth_status))
        .route("/auth/logout", post(auth::auth_logout))
        .route("/auth/exchange", post(auth::auth_code_exchange))
        // Dev auth (only active when RELAY_DEV_MODE=true)
        .route("/auth/dev", get(auth::dev_login).post(auth::dev_login_post))
        .route("/auth/dev/token", post(auth::dev_token))
        // Device code flow (for headless/remote login)
        .route("/auth/device", post(auth::device_code_start))
        .route("/auth/device/poll", post(auth::device_code_poll))
        .route("/auth/device/verify", post(auth::device_code_verify))
        // Device code verification page
        .route("/device", get(static_files::serve_device_page))
        // WebSocket endpoints
        .route("/ws/client", get(ws_client_upgrade))
        .route("/ws/daemon", get(ws_daemon_upgrade))
        // E2E encrypted REST tunnel
        .route("/api/_e2e", post(rest_tunnel::e2e_rest_tunnel_handler))
        // REST tunnel — catch-all for /api/*
        .route("/api/{*path}", any(rest_tunnel::rest_tunnel_handler))
        // Health check
        .route("/health", get(health))
        // List user's connected devices (requires auth)
        .route("/devices", get(list_devices))
        // Status (requires auth)
        .route("/status", get(status))
        // Static files (SPA fallback)
        .fallback(get(static_files::serve_static))
        .layer(cors)
        .with_state(state)
}

/// GET /health
async fn health() -> &'static str {
    "ok"
}

/// GET /devices — list the user's connected devices.
async fn list_devices(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let token = auth::extract_token_from_cookie(&headers).or_else(|| {
        headers
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
            return (StatusCode::UNAUTHORIZED, "Authentication required").into_response();
        }
    };

    let devices = state.list_devices(&claims.sub);
    axum::Json(serde_json::json!({ "devices": devices })).into_response()
}

/// GET /devices/names — simple list of device names (used by /auth/status).
async fn list_device_names(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let token = auth::extract_token_from_cookie(&headers).or_else(|| {
        headers
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
            return (StatusCode::UNAUTHORIZED, "Authentication required").into_response();
        }
    };

    let devices = state.list_device_names(&claims.sub);
    axum::Json(serde_json::json!({ "devices": devices })).into_response()
}

/// GET /status — relay connection overview (requires browser cookie auth)
async fn status(State(state): State<SharedState>, headers: axum::http::HeaderMap) -> Response {
    // Require browser cookie authentication
    let token = match auth::extract_token_from_cookie(&headers) {
        Some(t) => t,
        None => return (StatusCode::UNAUTHORIZED, "Authentication required").into_response(),
    };
    if auth::verify_jwt(&state.browser_jwt_secret(), &token).is_err() {
        return (StatusCode::UNAUTHORIZED, "Invalid or expired session").into_response();
    }

    let daemon_count = state.daemon_connections.len();
    let browser_count = state.browser_connections.len();
    let pending_rest = state.pending_rest.len();

    axum::Json(serde_json::json!({
        "daemons": daemon_count,
        "browsers": browser_count,
        "pendingRestRequests": pending_rest,
    }))
    .into_response()
}

/// WebSocket upgrade for browser clients.
/// Reads JWT from the HttpOnly `lukan_token` cookie (set during login).
/// Requires `?device=<name>` query parameter to select which daemon to connect to.
async fn ws_client_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: axum::http::HeaderMap,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let token = match auth::extract_token_from_cookie(&headers) {
        Some(t) => t,
        None => return (StatusCode::UNAUTHORIZED, "Missing authentication cookie").into_response(),
    };

    let claims = match auth::verify_jwt(&state.browser_jwt_secret(), &token) {
        Ok(c) => c,
        Err(e) => return (StatusCode::UNAUTHORIZED, format!("Invalid token: {e}")).into_response(),
    };

    let device = match params.get("device") {
        Some(d) => d.clone(),
        None => return (StatusCode::BAD_REQUEST, "Missing device parameter").into_response(),
    };

    if !state.has_daemon(&claims.sub, &device) {
        return (StatusCode::NOT_FOUND, "Device not connected").into_response();
    }

    let ip = auth::client_ip(&headers, Some(&ConnectInfo(addr))).to_string();

    ws.on_upgrade(move |socket| {
        ws_client::handle_browser_ws(socket, state, claims.sub, device, ip)
    })
}

/// WebSocket upgrade for daemon connections.
/// Requires `Authorization: Bearer <jwt>` header (daemon tokens use base jwt_secret).
async fn ws_daemon_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Response {
    // Extract token from Authorization header only (never from query string to avoid URL logging)
    let token = match headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    {
        Some(t) => t.to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing Authorization header. Use: Authorization: Bearer <token>",
            )
                .into_response()
        }
    };

    // Daemon tokens use base jwt_secret (survive relay restarts)
    let claims = match auth::verify_jwt(&state.jwt_secret, &token) {
        Ok(c) => c,
        Err(e) => return (StatusCode::UNAUTHORIZED, format!("Invalid token: {e}")).into_response(),
    };

    ws.on_upgrade(move |socket| ws_daemon::handle_daemon_ws(socket, state, claims.sub))
}
