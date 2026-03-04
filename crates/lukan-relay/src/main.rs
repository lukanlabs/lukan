#![allow(dead_code)]

mod auth;
mod rest_tunnel;
mod state;
mod static_files;
mod ws_client;
mod ws_daemon;

use std::sync::Arc;

use axum::Router;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use tower_http::cors::CorsLayer;
use tracing::info;

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

    if state.dev_mode {
        info!("DEV MODE enabled — /auth/dev endpoints are active (do NOT use in production)");
    }
    info!("Boot ID: {} (browser tokens from previous boots are now invalid)", &state.boot_id[..8]);

    let router = create_router(state.clone());

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let addr = format!("0.0.0.0:{port}");
    info!("lukan-relay listening on {addr}");
    info!("Public URL: {}", state.public_url);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}

fn create_router(state: SharedState) -> Router {
    // Permissive CORS — the SPA is same-origin so cookies work without CORS.
    // This layer is mainly for potential external API consumers.
    let cors = CorsLayer::permissive();

    Router::new()
        // Auth
        .route("/auth/google", get(auth::google_login))
        .route("/auth/callback", get(auth::google_callback))
        .route("/auth/status", get(auth::auth_status))
        .route("/auth/logout", post(auth::auth_logout))
        // Dev auth (only active when RELAY_DEV_MODE=true)
        .route("/auth/dev", get(auth::dev_login).post(auth::dev_login_post))
        .route("/auth/dev/token", post(auth::dev_token))
        // WebSocket endpoints
        .route("/ws/client", get(ws_client_upgrade))
        .route("/ws/daemon", get(ws_daemon_upgrade))
        // E2E encrypted REST tunnel
        .route("/api/_e2e", post(rest_tunnel::e2e_rest_tunnel_handler))
        // REST tunnel — catch-all for /api/*
        .route("/api/{*path}", any(rest_tunnel::rest_tunnel_handler))
        // Health check
        .route("/health", get(health))
        // Status (shows connected daemons/browsers)
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

/// GET /status — relay connection overview (for debugging)
async fn status(State(state): State<SharedState>) -> impl IntoResponse {
    let daemon_count = state.daemon_connections.len();
    let browser_count = state.browser_connections.len();
    let pending_rest = state.pending_rest.len();

    axum::Json(serde_json::json!({
        "daemons": daemon_count,
        "browsers": browser_count,
        "pendingRestRequests": pending_rest,
    }))
}

/// WebSocket upgrade for browser clients.
/// Reads JWT from the HttpOnly `lukan_token` cookie (set during login).
async fn ws_client_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let token = match auth::extract_token_from_cookie(&headers) {
        Some(t) => t,
        None => return (StatusCode::UNAUTHORIZED, "Missing authentication cookie").into_response(),
    };

    let claims = match auth::verify_jwt(&state.browser_jwt_secret(), &token) {
        Ok(c) => c,
        Err(e) => return (StatusCode::UNAUTHORIZED, format!("Invalid token: {e}")).into_response(),
    };

    ws.on_upgrade(move |socket| ws_client::handle_browser_ws(socket, state, claims.sub))
}

/// WebSocket upgrade for daemon connections.
/// Requires `?token=<jwt>` query parameter (daemon tokens use base jwt_secret).
async fn ws_daemon_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let token = match params.get("token") {
        Some(t) => t.clone(),
        None => return (StatusCode::UNAUTHORIZED, "Missing token parameter").into_response(),
    };

    // Daemon tokens use base jwt_secret (survive relay restarts)
    let claims = match auth::verify_jwt(&state.jwt_secret, &token) {
        Ok(c) => c,
        Err(e) => return (StatusCode::UNAUTHORIZED, format!("Invalid token: {e}")).into_response(),
    };

    ws.on_upgrade(move |socket| ws_daemon::handle_daemon_ws(socket, state, claims.sub))
}
