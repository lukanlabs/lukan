use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Json},
    routing::{delete, get, post, put},
};
use tower_http::cors::{Any, CorsLayer};

use crate::auth_middleware::require_auth;
use crate::state::AppState;
use crate::static_files;
use crate::ws_handler::ws_upgrade_handler;
use crate::{
    rest_auto, rest_browser, rest_config, rest_credentials, rest_events, rest_files, rest_memory,
    rest_pipelines, rest_plugins, rest_processes, rest_providers, rest_workers,
};

/// Build the Axum router with all routes
pub fn create_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // API routes that require authentication
    let api_routes = Router::new()
        // Config
        .route("/config", get(rest_config::get_config))
        .route("/config", put(rest_config::save_config))
        .route("/config/{key}", get(rest_config::get_config_value))
        .route("/config/{key}", put(rest_config::set_config_value))
        .route("/tools", get(rest_config::list_tools))
        // Credentials
        .route("/credentials", get(rest_credentials::get_credentials))
        .route("/credentials", put(rest_credentials::save_credentials))
        // Providers
        .route("/providers", get(rest_providers::list_providers))
        .route(
            "/providers/status",
            get(rest_credentials::get_provider_status),
        )
        .route(
            "/providers/active",
            put(rest_providers::set_active_provider),
        )
        .route(
            "/providers/{name}/test",
            post(rest_credentials::test_provider),
        )
        .route(
            "/providers/{name}/models",
            get(rest_providers::fetch_provider_models),
        )
        .route(
            "/providers/{name}/models",
            put(rest_providers::set_provider_models),
        )
        .route("/models", get(rest_providers::get_models))
        .route("/models", post(rest_providers::add_model))
        // Plugins
        .route("/plugins", get(rest_plugins::list_plugins))
        .route("/plugins/install", post(rest_plugins::install_plugin))
        .route(
            "/plugins/install-remote",
            post(rest_plugins::install_remote_plugin),
        )
        .route("/plugins/remote", get(rest_plugins::list_remote_plugins))
        .route(
            "/plugins/{name}/auth/qr",
            get(rest_plugins::get_plugin_auth_qr),
        )
        .route(
            "/plugins/{name}/auth/status",
            get(rest_plugins::check_plugin_auth),
        )
        .route("/plugins/{name}", delete(rest_plugins::remove_plugin))
        .route("/plugins/{name}/start", post(rest_plugins::start_plugin))
        .route("/plugins/{name}/stop", post(rest_plugins::stop_plugin))
        .route(
            "/plugins/{name}/restart",
            post(rest_plugins::restart_plugin),
        )
        .route(
            "/plugins/{name}/config",
            get(rest_plugins::get_plugin_config),
        )
        .route(
            "/plugins/{name}/config",
            put(rest_plugins::set_plugin_config_field),
        )
        .route("/plugins/{name}/logs", get(rest_plugins::get_plugin_logs))
        .route(
            "/plugins/{name}/commands",
            get(rest_plugins::get_plugin_commands),
        )
        .route(
            "/plugins/{name}/commands/{command}",
            post(rest_plugins::run_plugin_command),
        )
        .route(
            "/plugins/{name}/manifest-info",
            get(rest_plugins::get_plugin_manifest_info),
        )
        .route(
            "/plugins/{name}/tools",
            get(rest_plugins::get_plugin_manifest_tools),
        )
        .route(
            "/plugins/{name}/views/{view_id}",
            get(rest_plugins::get_plugin_view_data),
        )
        .route(
            "/plugins/{name}/web/{*path}",
            get(rest_plugins::serve_plugin_web),
        )
        // Memory
        .route("/memory/global", get(rest_memory::get_global_memory))
        .route("/memory/global", put(rest_memory::save_global_memory))
        .route("/memory/project", get(rest_memory::get_project_memory))
        .route("/memory/project", put(rest_memory::save_project_memory))
        .route(
            "/memory/project/active",
            get(rest_memory::is_project_memory_active),
        )
        .route(
            "/memory/project/active",
            put(rest_memory::toggle_project_memory),
        )
        // Events
        .route("/events/consume", post(rest_events::consume_pending_events))
        .route("/events/history", get(rest_events::get_event_history))
        .route("/events/history", delete(rest_events::clear_event_history))
        // Files
        .route("/files", get(rest_files::list_directory))
        .route("/files/read", get(rest_files::read_file))
        .route("/files/write", put(rest_files::write_file))
        .route("/cwd", get(rest_files::get_cwd))
        .route("/terminal/{id}/cwd", get(rest_files::get_terminal_cwd))
        .route("/git", get(rest_files::git_command))
        .route(
            "/active-tab",
            axum::routing::post(rest_files::set_active_tab),
        )
        // Background processes
        .route("/processes", get(rest_processes::list_bg_processes))
        .route(
            "/processes/background",
            post(rest_processes::send_to_background),
        )
        .route(
            "/processes/clear",
            post(rest_processes::clear_completed_processes),
        )
        .route(
            "/processes/{pid}/log",
            get(rest_processes::get_bg_process_log),
        )
        .route(
            "/processes/{pid}/kill",
            post(rest_processes::kill_bg_process),
        )
        // Browser
        .route("/browser/launch", post(rest_browser::browser_launch))
        .route("/browser/status", get(rest_browser::browser_status))
        .route("/browser/navigate", post(rest_browser::browser_navigate))
        .route("/browser/screenshot", get(rest_browser::browser_screenshot))
        .route("/browser/tabs", get(rest_browser::browser_tabs))
        .route("/browser/close", post(rest_browser::browser_close))
        // Workers
        .route("/workers", get(rest_workers::list_workers))
        .route("/workers", post(rest_workers::create_worker))
        .route("/workers/{id}", get(rest_workers::get_worker_detail))
        .route("/workers/{id}", put(rest_workers::update_worker))
        .route("/workers/{id}", delete(rest_workers::delete_worker))
        .route("/workers/{id}/toggle", put(rest_workers::toggle_worker))
        .route(
            "/workers/{id}/runs/{run_id}",
            get(rest_workers::get_worker_run),
        )
        // Pipeline approvals (registered before /pipelines/{id} to avoid "approvals" matching as ID)
        .route(
            "/pipelines/approvals/pending",
            get(rest_pipelines::list_pending_approvals),
        )
        .route(
            "/pipelines/approvals/{id}/approve",
            post(rest_pipelines::approve_approval),
        )
        .route(
            "/pipelines/approvals/{id}/reject",
            post(rest_pipelines::reject_approval),
        )
        // Pipelines
        .route("/pipelines", get(rest_pipelines::list_pipelines))
        .route("/pipelines", post(rest_pipelines::create_pipeline))
        .route("/pipelines/{id}", get(rest_pipelines::get_pipeline_detail))
        .route("/pipelines/{id}", put(rest_pipelines::update_pipeline))
        .route("/pipelines/{id}", delete(rest_pipelines::delete_pipeline))
        .route(
            "/pipelines/{id}/toggle",
            put(rest_pipelines::toggle_pipeline),
        )
        .route(
            "/pipelines/{id}/trigger",
            post(rest_pipelines::trigger_pipeline),
        )
        .route(
            "/pipelines/{id}/runs/{run_id}",
            get(rest_pipelines::get_pipeline_run),
        )
        .route(
            "/pipelines/{id}/webhook",
            post(rest_pipelines::webhook_pipeline),
        )
        .route(
            "/pipelines/{id}/cancel",
            post(rest_pipelines::cancel_pipeline),
        )
        // Audio transcription
        .route(
            "/transcription/status",
            get(rest_plugins::check_transcription_status),
        )
        .route(
            "/transcription/transcribe",
            post(rest_plugins::transcribe_audio),
        )
        // Autonomous agent (cloud-agent relay trigger)
        .route("/auto/run", post(rest_auto::start_auto_run))
        .route("/auto/jobs/:id", get(rest_auto::get_auto_job))
        .layer(middleware::from_fn_with_state(state.clone(), require_auth));

    Router::new()
        .route("/ws", get(ws_upgrade_handler))
        .route("/api/auth", post(auth_handler))
        .route("/api/auth/status", get(auth_status_handler))
        .route("/health", get(health_handler))
        .route("/approve/{id}", get(rest_pipelines::approval_page))
        .route(
            "/api/pipelines/approvals/{id}/page",
            get(rest_pipelines::approval_page),
        )
        .nest("/api", api_routes)
        .fallback(get(static_files::serve_static))
        .layer(cors)
        .with_state(state)
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
