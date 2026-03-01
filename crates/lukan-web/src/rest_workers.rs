use axum::{Json, extract::Path, http::StatusCode, response::IntoResponse};
use lukan_core::workers::{WorkerCreateInput, WorkerManager, WorkerUpdateInput};

/// GET /api/workers
pub async fn list_workers() -> impl IntoResponse {
    match WorkerManager::get_summaries().await {
        Ok(summaries) => Json(summaries).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /api/workers
pub async fn create_worker(Json(input): Json<WorkerCreateInput>) -> impl IntoResponse {
    match WorkerManager::create(input).await {
        Ok(worker) => Json(worker).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/workers/:id
pub async fn update_worker(
    Path(id): Path<String>,
    Json(patch): Json<WorkerUpdateInput>,
) -> impl IntoResponse {
    match WorkerManager::update(&id, patch).await {
        Ok(Some(worker)) => Json(worker).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, format!("Worker '{id}' not found")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// DELETE /api/workers/:id
pub async fn delete_worker(Path(id): Path<String>) -> impl IntoResponse {
    match WorkerManager::delete(&id).await {
        Ok(deleted) => Json(deleted).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/workers/:id/toggle
pub async fn toggle_worker(
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let enabled = body["enabled"].as_bool().unwrap_or(false);
    let patch = WorkerUpdateInput {
        name: None,
        schedule: None,
        prompt: None,
        tools: None,
        provider: None,
        model: None,
        enabled: Some(enabled),
        notify: None,
    };
    match WorkerManager::update(&id, patch).await {
        Ok(Some(worker)) => Json(worker).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, format!("Worker '{id}' not found")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/workers/:id
pub async fn get_worker_detail(Path(id): Path<String>) -> impl IntoResponse {
    match WorkerManager::get_detail(&id).await {
        Ok(Some(detail)) => Json(detail).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, format!("Worker '{id}' not found")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/workers/:id/runs/:runId
pub async fn get_worker_run(
    Path((worker_id, run_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match WorkerManager::get_run(&worker_id, &run_id).await {
        Ok(Some(run)) => Json(run).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            format!("Run '{run_id}' not found for worker '{worker_id}'"),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
