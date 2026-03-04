use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::state::AppState;

/// Axum middleware that checks `Authorization: Bearer <token>` header.
/// Skips auth if no web password is configured or if request comes from the
/// relay bridge (identified by `X-Relay-Internal` header, only reachable from localhost).
pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    // If no password set, allow everything
    if !state.auth_required() {
        return next.run(req).await;
    }

    // Skip auth for relay bridge requests (the relay bridge runs on localhost
    // and injects this header when tunneling REST requests from the cloud relay)
    if req.headers().get("x-relay-internal").is_some() {
        return next.run(req).await;
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
