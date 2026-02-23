use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
};
use tower_http::cors::{Any, CorsLayer};

use crate::embedded_ui::EMBEDDED_HTML;
use crate::state::AppState;
use crate::ws_handler::ws_upgrade_handler;

/// Build the Axum router with all routes
pub fn create_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/ws", get(ws_upgrade_handler))
        .route("/api/auth", post(auth_handler))
        .route("/api/auth/status", get(auth_status_handler))
        .route("/health", get(health_handler))
        .fallback(get(serve_embedded_html))
        .layer(cors)
        .with_state(state)
}

/// Serve the embedded React SPA
async fn serve_embedded_html() -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        Html(EMBEDDED_HTML),
    )
}

/// POST /api/auth — validate password, return token
async fn auth_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let password = body.get("password").and_then(|v| v.as_str()).unwrap_or("");

    match state.validate_password(password) {
        Some(token) => Json(serde_json::json!({ "token": token })).into_response(),
        None => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Invalid password" })),
        )
            .into_response(),
    }
}

/// GET /api/auth/status — check if auth is required
async fn auth_status_handler(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "required": state.auth_required()
    }))
}

/// GET /health
async fn health_handler() -> &'static str {
    "ok"
}
