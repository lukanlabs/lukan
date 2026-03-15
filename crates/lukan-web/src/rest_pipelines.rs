use std::sync::Arc;

use axum::{Json, extract::Path, extract::State, http::StatusCode, response::IntoResponse};
use lukan_core::pipelines::{PipelineCreateInput, PipelineManager, PipelineUpdateInput};

use crate::state::AppState;

/// GET /api/pipelines
pub async fn list_pipelines() -> impl IntoResponse {
    match PipelineManager::get_summaries().await {
        Ok(summaries) => Json(summaries).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /api/pipelines
pub async fn create_pipeline(Json(input): Json<PipelineCreateInput>) -> impl IntoResponse {
    match PipelineManager::create(input).await {
        Ok(pipeline) => Json(pipeline).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/pipelines/:id
pub async fn update_pipeline(
    Path(id): Path<String>,
    Json(patch): Json<PipelineUpdateInput>,
) -> impl IntoResponse {
    match PipelineManager::update(&id, patch).await {
        Ok(Some(pipeline)) => Json(pipeline).into_response(),
        Ok(None) => {
            (StatusCode::NOT_FOUND, format!("Pipeline '{id}' not found")).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// DELETE /api/pipelines/:id
pub async fn delete_pipeline(Path(id): Path<String>) -> impl IntoResponse {
    match PipelineManager::delete(&id).await {
        Ok(deleted) => Json(deleted).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// PUT /api/pipelines/:id/toggle
pub async fn toggle_pipeline(
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let enabled = body["enabled"].as_bool().unwrap_or(false);
    let patch = PipelineUpdateInput {
        name: None,
        description: None,
        trigger: None,
        steps: None,
        connections: None,
        enabled: Some(enabled),
    };
    match PipelineManager::update(&id, patch).await {
        Ok(Some(pipeline)) => Json(pipeline).into_response(),
        Ok(None) => {
            (StatusCode::NOT_FOUND, format!("Pipeline '{id}' not found")).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/pipelines/:id
pub async fn get_pipeline_detail(Path(id): Path<String>) -> impl IntoResponse {
    match PipelineManager::get_detail(&id).await {
        Ok(Some(detail)) => Json(detail).into_response(),
        Ok(None) => {
            (StatusCode::NOT_FOUND, format!("Pipeline '{id}' not found")).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/pipelines/:id/runs/:runId
pub async fn get_pipeline_run(
    Path((pipeline_id, run_id)): Path<(String, String)>,
) -> impl IntoResponse {
    match PipelineManager::get_run(&pipeline_id, &run_id).await {
        Ok(Some(run)) => Json(run).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            format!("Run '{run_id}' not found for pipeline '{pipeline_id}'"),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /api/pipelines/:id/trigger
pub async fn trigger_pipeline(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let input = body["input"].as_str().map(|s| s.to_string());

    let pipeline = match PipelineManager::get(&id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, format!("Pipeline '{id}' not found")).into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    // Get config and spawn execution in background
    let config = state.config.lock().await.clone();
    let pipeline_notify_tx = state.pipeline_notification_tx.clone();

    tokio::spawn(async move {
        let run =
            lukan_agent::pipelines::executor::execute_pipeline(&pipeline, input, &config).await;

        // Emit notification
        let summary = if run.status == "success" {
            let step_count = run.step_runs.iter().filter(|s| s.status == "success").count();
            format!("{step_count} steps completed successfully")
        } else {
            let error_step = run.step_runs.iter().find(|s| s.status == "error");
            error_step
                .and_then(|s| s.error.clone())
                .unwrap_or_else(|| format!("Pipeline {}", run.status))
        };

        let notification = lukan_agent::PipelineNotification {
            pipeline_id: pipeline.id,
            pipeline_name: pipeline.name,
            status: run.status,
            summary,
        };
        let _ = pipeline_notify_tx.send(notification.clone());

        // Also write to JSONL file for other clients via NotificationWatcher
        if let Ok(line) = serde_json::to_string(&notification) {
            let path = lukan_core::config::LukanPaths::pipeline_notifications_file();
            if let Ok(mut file) = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
            {
                use tokio::io::AsyncWriteExt;
                let _ = file.write_all(format!("{line}\n").as_bytes()).await;
            }
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({
            "status": "triggered",
            "pipelineId": id,
        })),
    )
        .into_response()
}
