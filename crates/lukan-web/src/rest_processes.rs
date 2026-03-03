use axum::{
    Json,
    extract::{Path, Query},
    http::StatusCode,
    response::IntoResponse,
};
use lukan_tools::bg_processes::{self, BgProcessStatus};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BgProcessDto {
    pub pid: u32,
    pub command: String,
    pub started_at: String,
    pub exited_at: Option<String>,
    pub status: String,
    pub label: Option<String>,
    pub session_id: Option<String>,
    pub tab_id: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct SessionQuery {
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

/// GET /api/processes?sessionId=...
pub async fn list_bg_processes(Query(q): Query<SessionQuery>) -> Json<Vec<BgProcessDto>> {
    let procs = match q.session_id.as_deref() {
        Some(id) if !id.is_empty() => bg_processes::get_bg_processes_for_session(id),
        _ => bg_processes::get_bg_processes(),
    };

    Json(
        procs
            .into_iter()
            .map(|p| BgProcessDto {
                pid: p.pid,
                command: p.command,
                started_at: p.started_at.to_rfc3339(),
                exited_at: p.exited_at.map(|t| t.to_rfc3339()),
                status: match p.status {
                    BgProcessStatus::Running => "running".to_string(),
                    BgProcessStatus::Completed => "completed".to_string(),
                    BgProcessStatus::Killed => "killed".to_string(),
                },
                label: p.label,
                session_id: p.session_id,
                tab_id: p.tab_id,
            })
            .collect(),
    )
}

/// GET /api/processes/:pid/log?maxLines=N
pub async fn get_bg_process_log(
    Path(pid): Path<u32>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let max_lines: usize = params
        .get("maxLines")
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    match bg_processes::get_bg_log(pid, max_lines) {
        Some(log) => Json(serde_json::json!(log)).into_response(),
        None => Json(serde_json::Value::Null).into_response(),
    }
}

/// POST /api/processes/:pid/kill
pub async fn kill_bg_process(Path(pid): Path<u32>) -> Json<bool> {
    let sent = bg_processes::kill_bg_process(pid);
    if sent {
        bg_processes::kill_process_group_force(pid).await;
    }
    Json(sent)
}

/// POST /api/processes/background
pub async fn send_to_background() -> impl IntoResponse {
    // In web mode, we don't have access to the bg_signal_tx from ChatState.
    // This would require access to the shared AppState.
    // For now, return false as this is a Tauri-specific feature.
    (StatusCode::OK, Json(serde_json::json!(false)))
}
