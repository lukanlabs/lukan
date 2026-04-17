use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::state::AppState;

/// Axum middleware that checks `Authorization: Bearer <token>` header.
/// Skips auth if no web password is configured or if request comes from the
/// relay bridge (identified by `X-Relay-Internal` header AND loopback peer).
pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    // If no password set, allow everything
    if !state.auth_required() {
        return next.run(req).await;
    }

    // Skip auth for relay bridge requests — but verify the peer is loopback.
    // The relay bridge runs in-process on 127.0.0.1, so legitimate tunneled
    // REST requests always originate from a loopback address. Without this
    // check, any LAN attacker could send `x-relay-internal: true` and bypass
    // authentication entirely when the daemon binds to 0.0.0.0.
    if req.headers().get("x-relay-internal").is_some() {
        let peer_is_loopback = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip().is_loopback())
            .unwrap_or(false);
        if peer_is_loopback {
            return next.run(req).await;
        }
        // Spoofed header from non-loopback peer → treat as unauthorized
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Unauthorized" })),
        )
            .into_response();
    }

    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match token {
        Some(t) if state.verify_token(t) => next.run(req).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "Unauthorized" })),
        )
            .into_response(),
    }
}
