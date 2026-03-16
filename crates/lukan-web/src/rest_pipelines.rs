use std::sync::Arc;

use axum::{
    Json, extract::Path, extract::Query, extract::State, http::StatusCode, response::IntoResponse,
};
use lukan_core::approvals::ApprovalManager;
use lukan_core::pipelines::{
    PipelineCreateInput, PipelineManager, PipelineTrigger, PipelineUpdateInput,
};
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

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
        Ok(None) => (StatusCode::NOT_FOUND, format!("Pipeline '{id}' not found")).into_response(),
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
        Ok(None) => (StatusCode::NOT_FOUND, format!("Pipeline '{id}' not found")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/pipelines/:id
pub async fn get_pipeline_detail(Path(id): Path<String>) -> impl IntoResponse {
    match PipelineManager::get_detail(&id).await {
        Ok(Some(detail)) => Json(detail).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, format!("Pipeline '{id}' not found")).into_response(),
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
    let cancel_tokens = Arc::clone(&state.pipeline_cancel_tokens);
    let cancel_token = CancellationToken::new();
    cancel_tokens
        .lock()
        .await
        .insert(id.clone(), cancel_token.clone());
    let pipeline_id_for_cleanup = id.clone();

    let run_notify_tx = pipeline_notify_tx.clone();

    tokio::spawn(async move {
        let run = lukan_agent::pipelines::executor::execute_pipeline_full(
            &pipeline,
            input,
            &config,
            cancel_token,
            Some(run_notify_tx),
        )
        .await;

        // Emit completion notification
        let summary = if run.status == "success" {
            let step_count = run
                .step_runs
                .iter()
                .filter(|s| s.status == "success")
                .count();
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

        // Cleanup cancel token
        cancel_tokens.lock().await.remove(&pipeline_id_for_cleanup);
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

/// POST /api/pipelines/:id/cancel — cancel a running pipeline
pub async fn cancel_pipeline(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let tokens = state.pipeline_cancel_tokens.lock().await;
    if let Some(token) = tokens.get(&id) {
        token.cancel();
        (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "cancelled", "pipelineId": id })),
        )
            .into_response()
    } else {
        let active_ids: Vec<String> = tokens.keys().cloned().collect();
        tracing::warn!(
            pipeline_id = %id,
            active_tokens = ?active_ids,
            "Cancel requested but no cancel token found"
        );
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "status": "not_running",
                "pipelineId": id,
                "activeTokens": active_ids,
            })),
        )
            .into_response()
    }
}

#[derive(Deserialize)]
pub struct WebhookQuery {
    secret: Option<String>,
}

/// POST /api/pipelines/:id/webhook — public webhook endpoint
pub async fn webhook_pipeline(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<WebhookQuery>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let pipeline = match PipelineManager::get(&id).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, format!("Pipeline '{id}' not found")).into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    // Verify this pipeline has a webhook trigger
    let expected_secret = match &pipeline.trigger {
        PipelineTrigger::Webhook { secret } => secret.clone(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "Pipeline is not configured with a webhook trigger",
            )
                .into_response();
        }
    };

    // Validate secret if configured
    if let Some(ref expected) = expected_secret {
        let provided = query.secret.as_deref().unwrap_or("");
        if provided != expected {
            return (StatusCode::UNAUTHORIZED, "Invalid webhook secret").into_response();
        }
    }

    // Use the request body as the trigger input
    let input = Some(serde_json::to_string_pretty(&body).unwrap_or_default());

    let config = state.config.lock().await.clone();
    let pipeline_notify_tx = state.pipeline_notification_tx.clone();

    tokio::spawn(async move {
        let run =
            lukan_agent::pipelines::executor::execute_pipeline(&pipeline, input, &config).await;

        let summary = if run.status == "success" {
            let step_count = run
                .step_runs
                .iter()
                .filter(|s| s.status == "success")
                .count();
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
            "source": "webhook",
        })),
    )
        .into_response()
}

// ── Approval endpoints ──────────────────────────────────────────────

/// GET /api/pipelines/approvals/pending — list pending approvals
pub async fn list_pending_approvals() -> impl IntoResponse {
    match ApprovalManager::list_pending().await {
        Ok(approvals) => Json(approvals).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
pub struct ApprovalAction {
    pub comment: Option<String>,
}

/// POST /api/pipelines/approvals/:id/approve
pub async fn approve_approval(
    Path(id): Path<String>,
    Json(body): Json<ApprovalAction>,
) -> impl IntoResponse {
    match ApprovalManager::resolve(&id, true, "ui", body.comment).await {
        Ok(Some(req)) => Json(req).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, format!("Approval '{id}' not found")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// POST /api/pipelines/approvals/:id/reject
pub async fn reject_approval(
    Path(id): Path<String>,
    Json(body): Json<ApprovalAction>,
) -> impl IntoResponse {
    match ApprovalManager::resolve(&id, false, "ui", body.comment).await {
        Ok(Some(req)) => Json(req).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, format!("Approval '{id}' not found")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
