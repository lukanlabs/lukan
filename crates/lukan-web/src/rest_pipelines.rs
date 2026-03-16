use std::sync::Arc;

use axum::{
    Json,
    extract::Path,
    extract::Query,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
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

/// GET /approve/:id — public HTML page for approval (login-gated via JS)
pub async fn approval_page(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let auth_required = state.auth_required();

    let approval = match ApprovalManager::get(&id).await {
        Ok(Some(req)) => req,
        Ok(None) => {
            return Html(format!(
                r#"<!DOCTYPE html><html><head><title>Approval Not Found</title>
                <meta name="viewport" content="width=device-width,initial-scale=1">
                <style>{}</style></head>
                <body><div class="card"><h2>Approval not found</h2><p>This approval may have expired or already been resolved.</p></div></body></html>"#,
                APPROVAL_CSS
            ))
            .into_response();
        }
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let pipeline_name = PipelineManager::get(&approval.pipeline_id)
        .await
        .ok()
        .flatten()
        .map(|p| p.name)
        .unwrap_or_else(|| approval.pipeline_id.clone());

    let context_escaped = approval
        .context
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\n', "<br>");

    let approved_msg = format!(
        "Approved{}",
        approval
            .resolved_by
            .as_deref()
            .map(|r| format!(" by {r}"))
            .unwrap_or_default()
    );
    let rejected_msg = format!(
        "Rejected{}",
        approval
            .resolved_by
            .as_deref()
            .map(|r| format!(" by {r}"))
            .unwrap_or_default()
    );

    let (status_class, status_msg, buttons) = match approval.status.as_str() {
        "pending" => (
            "pending",
            "Waiting for your decision",
            r#"<div class="buttons">
                    <button class="btn approve" onclick="resolve('approve')">Approve</button>
                    <button class="btn reject" onclick="resolve('reject')">Reject</button>
                </div>"#
                .to_string(),
        ),
        "approved" => ("approved", approved_msg.as_str(), String::new()),
        "rejected" => ("rejected", rejected_msg.as_str(), String::new()),
        s => (s, s, String::new()),
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html>
<head>
    <title>Pipeline Approval — {pipeline_name}</title>
    <meta name="viewport" content="width=device-width,initial-scale=1">
    <style>{APPROVAL_CSS}</style>
</head>
<body>
    <div id="login-gate" style="display:none">
        <div class="card">
            <h2>Login Required</h2>
            <p>Enter your password to view this approval.</p>
            <input type="password" id="password" placeholder="Password" onkeydown="if(event.key==='Enter')login()">
            <button class="btn approve" onclick="login()" style="margin-top:8px;width:100%">Login</button>
            <div id="login-error" class="error" style="display:none"></div>
        </div>
    </div>
    <div id="approval-content" style="display:none">
        <div class="card">
            <div class="header">
                <div class="pipeline-name">{pipeline_name}</div>
                <div class="status {status_class}">{status_msg}</div>
            </div>
            <div class="context">{context_escaped}</div>
            {buttons}
            <div id="result" class="result" style="display:none"></div>
        </div>
    </div>
    <script>
        const approvalId = "{id}";
        const authRequired = {auth_required};
        let token = localStorage.getItem("lukan-token");

        async function init() {{
            if (!authRequired) {{ show("approval-content"); return; }}
            if (token) {{
                const ok = await checkAuth();
                if (ok) {{ show("approval-content"); return; }}
            }}
            show("login-gate");
        }}

        async function checkAuth() {{
            try {{
                const r = await fetch("/api/pipelines/approvals/pending", {{
                    headers: {{ "Authorization": "Bearer " + token }}
                }});
                return r.ok;
            }} catch {{ return false; }}
        }}

        async function login() {{
            const pw = document.getElementById("password").value;
            try {{
                const r = await fetch("/api/auth", {{
                    method: "POST",
                    headers: {{ "Content-Type": "application/json" }},
                    body: JSON.stringify({{ password: pw }})
                }});
                if (r.ok) {{
                    const data = await r.json();
                    token = data.token;
                    localStorage.setItem("lukan-token", token);
                    hide("login-gate");
                    show("approval-content");
                }} else {{
                    document.getElementById("login-error").textContent = "Invalid password";
                    document.getElementById("login-error").style.display = "block";
                }}
            }} catch (e) {{
                document.getElementById("login-error").textContent = e.message;
                document.getElementById("login-error").style.display = "block";
            }}
        }}

        async function resolve(action) {{
            const btns = document.querySelectorAll(".btn");
            btns.forEach(b => {{ b.disabled = true; b.style.opacity = "0.5"; }});
            try {{
                const headers = {{ "Content-Type": "application/json" }};
                if (token) headers["Authorization"] = "Bearer " + token;
                const r = await fetch("/api/pipelines/approvals/" + approvalId + "/" + action, {{
                    method: "POST", headers, body: "{{}}"
                }});
                const el = document.getElementById("result");
                if (r.ok) {{
                    el.textContent = action === "approve" ? "Approved successfully" : "Rejected";
                    el.className = "result " + (action === "approve" ? "approved" : "rejected");
                }} else {{
                    el.textContent = "Failed: " + (await r.text());
                    el.className = "result rejected";
                }}
                el.style.display = "block";
                document.querySelector(".buttons").style.display = "none";
            }} catch (e) {{
                alert("Error: " + e.message);
                btns.forEach(b => {{ b.disabled = false; b.style.opacity = "1"; }});
            }}
        }}

        function show(id) {{ document.getElementById(id).style.display = "block"; }}
        function hide(id) {{ document.getElementById(id).style.display = "none"; }}
        init();
    </script>
</body>
</html>"#
    );

    Html(html).into_response()
}

const APPROVAL_CSS: &str = r#"
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body { background: #0a0a0a; color: #e0e0e0; font-family: -apple-system, system-ui, sans-serif; display: flex; justify-content: center; align-items: center; min-height: 100vh; padding: 16px; }
    .card { background: #141414; border: 1px solid #2a2a2a; border-radius: 12px; padding: 24px; max-width: 540px; width: 100%; }
    .header { margin-bottom: 16px; }
    .pipeline-name { font-size: 18px; font-weight: 600; color: #e0e0e0; margin-bottom: 4px; }
    .status { font-size: 13px; padding: 4px 10px; border-radius: 6px; display: inline-block; }
    .status.pending { background: rgba(139,92,246,0.15); color: #a78bfa; }
    .status.approved { background: rgba(34,197,94,0.15); color: #22c55e; }
    .status.rejected { background: rgba(239,68,68,0.15); color: #ef4444; }
    .context { font-size: 13px; color: #aaa; line-height: 1.6; padding: 16px; background: #0e0e0e; border: 1px solid #1e1e1e; border-radius: 8px; margin-bottom: 16px; max-height: 300px; overflow-y: auto; word-break: break-word; }
    .buttons { display: flex; gap: 10px; }
    .btn { flex: 1; padding: 10px; border: none; border-radius: 8px; font-size: 14px; font-weight: 600; cursor: pointer; transition: opacity 0.2s; }
    .btn:hover { opacity: 0.85; }
    .btn.approve { background: #22c55e; color: #fff; }
    .btn.reject { background: #ef4444; color: #fff; }
    .result { margin-top: 12px; padding: 10px; border-radius: 8px; font-size: 14px; font-weight: 500; text-align: center; }
    .result.approved { background: rgba(34,197,94,0.15); color: #22c55e; }
    .result.rejected { background: rgba(239,68,68,0.15); color: #ef4444; }
    .error { color: #ef4444; font-size: 12px; margin-top: 8px; }
    input { width: 100%; padding: 10px; background: #0e0e0e; border: 1px solid #2a2a2a; border-radius: 8px; color: #e0e0e0; font-size: 14px; outline: none; }
    input:focus { border-color: #8b5cf6; }
"#;
