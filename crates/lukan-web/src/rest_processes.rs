use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use lukan_tools::bg_processes::{self, BgProcessStatus};
use serde::Serialize;

use crate::state::AppState;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bg_process_dto_serialization() {
        let dto = BgProcessDto {
            pid: 12345,
            command: "cargo build".into(),
            started_at: "2024-01-01T00:00:00Z".into(),
            exited_at: None,
            status: "running".into(),
            label: Some("build".into()),
            session_id: Some("sess-1".into()),
            tab_id: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains(r#""pid":12345"#), "pid: {json}");
        assert!(
            json.contains(r#""startedAt""#),
            "startedAt camelCase: {json}"
        );
        assert!(json.contains(r#""exitedAt""#), "exitedAt camelCase: {json}");
        assert!(
            json.contains(r#""sessionId""#),
            "sessionId camelCase: {json}"
        );
        assert!(json.contains(r#""tabId""#), "tabId camelCase: {json}");
        assert!(!json.contains("started_at"), "no snake_case: {json}");
        assert!(!json.contains("exited_at"), "no snake_case: {json}");
        assert!(!json.contains("session_id"), "no snake_case: {json}");
        assert!(!json.contains("tab_id"), "no snake_case: {json}");
    }

    #[test]
    fn test_session_query_deserialize() {
        let q: SessionQuery = serde_json::from_str(r#"{"sessionId":"abc"}"#).unwrap();
        assert_eq!(q.session_id, Some("abc".to_string()));
    }

    #[test]
    fn test_session_query_deserialize_empty() {
        let q: SessionQuery = serde_json::from_str(r#"{}"#).unwrap();
        assert!(q.session_id.is_none());
    }
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

/// POST /api/processes/clear — remove all completed/killed processes from history
pub async fn clear_completed_processes() -> Json<serde_json::Value> {
    let removed = bg_processes::clear_completed();
    Json(serde_json::json!({ "removed": removed }))
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
pub async fn send_to_background(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut sent = false;

    // Check all sessions for an active bg_signal_tx
    {
        let sessions = state.sessions.lock().await;
        for session in sessions.values() {
            if let Some(ref tx) = session.bg_signal_tx
                && tx.send(()).is_ok()
            {
                sent = true;
                break;
            }
        }
    }

    (StatusCode::OK, Json(serde_json::json!(sent)))
}
