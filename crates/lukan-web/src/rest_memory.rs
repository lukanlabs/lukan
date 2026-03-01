use axum::{Json, extract::Query, http::StatusCode, response::IntoResponse};
use lukan_core::config::LukanPaths;

#[derive(serde::Deserialize)]
pub struct PathQuery {
    path: String,
}

/// GET /api/memory/global
pub async fn get_global_memory() -> impl IntoResponse {
    let path = LukanPaths::global_memory_file();
    if !path.exists() {
        return Json(serde_json::json!("")).into_response();
    }
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => Json(serde_json::json!(content)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/memory/global
pub async fn save_global_memory(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let content = body["content"].as_str().unwrap_or_default();
    let path = LukanPaths::global_memory_file();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    match tokio::fs::write(&path, content).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/memory/project?path=...
pub async fn get_project_memory(Query(q): Query<PathQuery>) -> impl IntoResponse {
    let memory_file = std::path::PathBuf::from(&q.path)
        .join(".lukan")
        .join("memories")
        .join("MEMORY.md");

    if !memory_file.exists() {
        return Json(serde_json::json!("")).into_response();
    }
    match tokio::fs::read_to_string(&memory_file).await {
        Ok(content) => Json(serde_json::json!(content)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/memory/project
pub async fn save_project_memory(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let path = body["path"].as_str().unwrap_or_default();
    let content = body["content"].as_str().unwrap_or_default();
    let memory_dir = std::path::PathBuf::from(path)
        .join(".lukan")
        .join("memories");
    let memory_file = memory_dir.join("MEMORY.md");

    if let Err(e) = tokio::fs::create_dir_all(&memory_dir).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }
    match tokio::fs::write(&memory_file, content).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/memory/project/active?path=...
pub async fn is_project_memory_active(Query(q): Query<PathQuery>) -> Json<bool> {
    let active_file = std::path::PathBuf::from(&q.path)
        .join(".lukan")
        .join("memories")
        .join(".active");
    Json(active_file.exists())
}

/// PUT /api/memory/project/active
pub async fn toggle_project_memory(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    let path = body["path"].as_str().unwrap_or_default();
    let active = body["active"].as_bool().unwrap_or(false);
    let memory_dir = std::path::PathBuf::from(path)
        .join(".lukan")
        .join("memories");
    let active_file = memory_dir.join(".active");

    if let Err(e) = tokio::fs::create_dir_all(&memory_dir).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    if active {
        match tokio::fs::write(&active_file, "").await {
            Ok(()) => StatusCode::OK.into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else if active_file.exists() {
        match tokio::fs::remove_file(&active_file).await {
            Ok(()) => StatusCode::OK.into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else {
        StatusCode::OK.into_response()
    }
}
