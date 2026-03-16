use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use rand::Rng;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use lukan_core::approvals::{ApprovalManager, ApprovalRequest};
use lukan_core::config::ResolvedConfig;
use lukan_core::models::events::StreamEvent;
use lukan_core::pipelines::{
    PipelineDefinition, PipelineManager, PipelineRun, PipelineTokenUsage, StepCondition, StepRun,
};
use lukan_providers::{SystemPrompt, create_provider};
use lukan_tools::create_configured_registry;

use crate::{AgentConfig, AgentLoop};

const MAX_RUNS_KEPT: usize = 20;

/// Execute a pipeline run with parallel execution within each topological level
pub async fn execute_pipeline(
    pipeline: &PipelineDefinition,
    trigger_input: Option<String>,
    config: &ResolvedConfig,
) -> PipelineRun {
    execute_pipeline_full(
        pipeline,
        trigger_input,
        config,
        CancellationToken::new(),
        None,
    )
    .await
}

/// Execute a pipeline run with cancellation support
pub async fn execute_pipeline_cancellable(
    pipeline: &PipelineDefinition,
    trigger_input: Option<String>,
    config: &ResolvedConfig,
    cancel_token: CancellationToken,
) -> PipelineRun {
    execute_pipeline_full(pipeline, trigger_input, config, cancel_token, None).await
}

/// Execute a pipeline run with cancellation and optional notification channel
pub async fn execute_pipeline_full(
    pipeline: &PipelineDefinition,
    trigger_input: Option<String>,
    config: &ResolvedConfig,
    cancel_token: CancellationToken,
    notify_tx: Option<tokio::sync::broadcast::Sender<crate::PipelineNotification>>,
) -> PipelineRun {
    let run_id = generate_run_id();
    info!(
        pipeline_id = %pipeline.id,
        run_id = %run_id,
        pipeline_name = %pipeline.name,
        "Starting pipeline run"
    );

    let mut run = PipelineRun {
        id: run_id,
        pipeline_id: pipeline.id.clone(),
        started_at: chrono::Utc::now().to_rfc3339(),
        completed_at: None,
        status: "running".to_string(),
        step_runs: pipeline
            .steps
            .iter()
            .map(|s| StepRun {
                step_id: s.id.clone(),
                step_name: s.name.clone(),
                status: "pending".to_string(),
                input: None,
                output: String::new(),
                error: None,
                started_at: None,
                completed_at: None,
                token_usage: PipelineTokenUsage::default(),
                turns: 0,
                approval_id: None,
            })
            .collect(),
        trigger_input: trigger_input.clone(),
        token_usage: PipelineTokenUsage::default(),
    };

    // Save initial "running" state
    if let Err(e) = PipelineManager::save_run(&run).await {
        error!(error = %e, "Failed to save initial pipeline run");
    }

    // Emit "started" notification (after save, so the run is on disk when frontend polls)
    if let Some(ref tx) = notify_tx {
        let _ = tx.send(crate::PipelineNotification {
            pipeline_id: pipeline.id.clone(),
            pipeline_name: pipeline.name.clone(),
            status: "running".to_string(),
            summary: "Pipeline started".to_string(),
        });
    }

    // Topological sort into levels (steps in the same level run in parallel)
    let levels = match topological_levels(pipeline) {
        Ok(lvls) => lvls,
        Err(e) => {
            run.status = "error".to_string();
            run.completed_at = Some(chrono::Utc::now().to_rfc3339());
            for sr in &mut run.step_runs {
                sr.status = "error".to_string();
                sr.error = Some(format!("Pipeline topology error: {e}"));
            }
            PipelineManager::save_run(&run).await.ok();
            PipelineManager::update_last_run(&pipeline.id, "error")
                .await
                .ok();
            return run;
        }
    };

    // Shared step outputs for template rendering (written by parallel tasks)
    let step_outputs: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

    // If there's trigger input, store it under "__trigger__"
    if let Some(ref input) = trigger_input {
        step_outputs
            .lock()
            .await
            .insert("__trigger__".to_string(), input.clone());
    }

    let mut has_error = false;
    // Track which steps failed (for downstream skipping)
    let mut failed_steps: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Shared run state for parallel progress saving
    let shared_run_state: Arc<Mutex<PipelineRun>> = Arc::new(Mutex::new(run.clone()));

    // Execute level by level
    for level in &levels {
        // Sync shared state with local run
        *shared_run_state.lock().await = run.clone();

        // Check for cancellation before each level
        if cancel_token.is_cancelled() {
            // Mark remaining pending/running/waiting steps as skipped
            for sr in &mut run.step_runs {
                if sr.status == "pending"
                    || sr.status == "running"
                    || sr.status == "waiting_approval"
                {
                    sr.status = "skipped".to_string();
                    sr.error = Some("Pipeline cancelled".to_string());
                }
            }
            has_error = true;
            break;
        }
        if level.len() == 1 {
            // Single step in level — run sequentially (no JoinSet overhead)
            let step_id = &level[0];
            has_error |= execute_single_step(
                pipeline,
                step_id,
                config,
                &mut run,
                &step_outputs,
                &mut failed_steps,
                &cancel_token,
            )
            .await;
        } else {
            // Multiple steps — run in parallel with JoinSet
            let mut join_set: JoinSet<StepResult> = JoinSet::new();

            for step_id in level {
                let step = pipeline
                    .steps
                    .iter()
                    .find(|s| s.id == *step_id)
                    .unwrap()
                    .clone();
                let step_run_idx = run
                    .step_runs
                    .iter()
                    .position(|sr| sr.step_id == *step_id)
                    .unwrap();

                // Snapshot outputs so far (read-only for this level)
                let outputs_snapshot = step_outputs.lock().await.clone();

                // Check if upstream conditions are met
                if !should_execute_step(pipeline, step_id, &outputs_snapshot, &run.step_runs) {
                    run.step_runs[step_run_idx].status = "skipped".to_string();
                    debug!(
                        step_id = step_id.as_str(),
                        "Step skipped (conditions not met)"
                    );
                    continue;
                }

                // Check if any upstream step failed with on_error=stop
                let upstream_failed = has_failed_upstream(pipeline, step_id, &failed_steps);
                if upstream_failed {
                    run.step_runs[step_run_idx].status = "skipped".to_string();
                    debug!(
                        step_id = step_id.as_str(),
                        "Step skipped (upstream failure)"
                    );
                    continue;
                }

                let input = gather_step_input(pipeline, step_id, &outputs_snapshot);

                // Approval steps in parallel: spawn a polling future
                if step.step_type == "approval" {
                    let step_clone = step.clone();
                    let sid = step_id.clone();
                    let ct = cancel_token.clone();
                    let shared_run = Arc::clone(&shared_run_state);
                    let outputs_snap = outputs_snapshot.clone();

                    run.step_runs[step_run_idx].input = input.clone();

                    join_set.spawn(async move {
                        let result = execute_approval_step_parallel(
                            &step_clone,
                            &input,
                            &outputs_snap,
                            &sid,
                            &shared_run,
                            &ct,
                        )
                        .await;
                        StepResult {
                            step_id: sid,
                            step_run_idx,
                            result,
                            on_error: step_clone.on_error.clone(),
                        }
                    });
                    continue;
                }

                let rendered_prompt = render_prompt(&step, &input, &outputs_snapshot);

                run.step_runs[step_run_idx].status = "running".to_string();
                run.step_runs[step_run_idx].input = input.clone();
                run.step_runs[step_run_idx].started_at = Some(chrono::Utc::now().to_rfc3339());

                let config_clone = config.clone();
                let sid = step_id.clone();
                let ct = cancel_token.clone();
                let shared_run = Arc::clone(&shared_run_state);

                join_set.spawn(async move {
                    // Live progress for this parallel step
                    let progress = Arc::new(Mutex::new(StepProgress::default()));
                    let progress_clone = Arc::clone(&progress);
                    let step_id_for_saver = sid.clone();
                    let shared_run_for_saver = Arc::clone(&shared_run);
                    let saver_cancel = CancellationToken::new();
                    let saver_cancel_clone = saver_cancel.clone();

                    // Background saver for this step
                    let saver = tokio::spawn(async move {
                        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
                        interval.tick().await;
                        loop {
                            tokio::select! {
                                _ = saver_cancel_clone.cancelled() => break,
                                _ = interval.tick() => {
                                    let p = progress_clone.lock().await;
                                    let mut r = shared_run_for_saver.lock().await;
                                    if let Some(sr) = r.step_runs.iter_mut().find(|s| s.step_id == step_id_for_saver) {
                                        sr.output = p.output.clone();
                                        sr.token_usage = p.token_usage.clone();
                                        sr.turns = p.turns;
                                        if !p.activity.is_empty() {
                                            sr.error = Some(format!("[activity] {}", p.activity));
                                        }
                                    }
                                    PipelineManager::save_run(&r).await.ok();
                                }
                            }
                        }
                    });

                    let result = execute_step_with_retry_live(
                        &step, &rendered_prompt, &config_clone, Some(&ct), Some(progress),
                    ).await;

                    saver_cancel.cancel();
                    saver.await.ok();

                    StepResult {
                        step_id: sid,
                        step_run_idx,
                        result,
                        on_error: step.on_error.clone(),
                    }
                });
            }

            // Save progress and sync shared state (all steps in level are now "running")
            PipelineManager::save_run(&run).await.ok();
            *shared_run_state.lock().await = run.clone();

            // Collect results from all parallel steps
            while let Some(join_result) = join_set.join_next().await {
                let sr = match join_result {
                    Ok(sr) => sr,
                    Err(e) => {
                        error!(error = %e, "Step task panicked");
                        has_error = true;
                        continue;
                    }
                };

                match sr.result {
                    Ok((clean_output, display_output, token_usage, turns)) => {
                        run.step_runs[sr.step_run_idx].status = "success".to_string();
                        run.step_runs[sr.step_run_idx].output = display_output; // UI sees tool log
                        run.step_runs[sr.step_run_idx].token_usage = token_usage.clone();
                        run.step_runs[sr.step_run_idx].turns = turns;
                        run.step_runs[sr.step_run_idx].completed_at =
                            Some(chrono::Utc::now().to_rfc3339());
                        run.step_runs[sr.step_run_idx].error = None; // clear activity

                        run.token_usage.input += token_usage.input;
                        run.token_usage.output += token_usage.output;
                        run.token_usage.cache_creation += token_usage.cache_creation;
                        run.token_usage.cache_read += token_usage.cache_read;

                        // Clean output (without tool log) goes to next step
                        step_outputs
                            .lock()
                            .await
                            .insert(sr.step_id.clone(), clean_output);
                    }
                    Err(e) => {
                        let error_msg = format!("{e}");
                        run.step_runs[sr.step_run_idx].status = "error".to_string();
                        run.step_runs[sr.step_run_idx].error = Some(error_msg.clone());
                        run.step_runs[sr.step_run_idx].completed_at =
                            Some(chrono::Utc::now().to_rfc3339());

                        let on_error = sr.on_error.as_deref().unwrap_or("stop");
                        if on_error == "skip" {
                            debug!(
                                step_id = sr.step_id.as_str(),
                                "Step error (skip): {}", error_msg
                            );
                            step_outputs
                                .lock()
                                .await
                                .insert(sr.step_id.clone(), String::new());
                        } else {
                            has_error = true;
                            failed_steps.insert(sr.step_id.clone());
                        }
                    }
                }

                // Save immediately after each step completes (don't wait for entire level)
                PipelineManager::save_run(&run).await.ok();
            }
        }

        // Save progress after each level
        PipelineManager::save_run(&run).await.ok();
    }

    // Determine final status
    let any_success = run.step_runs.iter().any(|sr| sr.status == "success");
    let any_error = run.step_runs.iter().any(|sr| sr.status == "error");

    run.status = if has_error && any_success {
        "partial".to_string()
    } else if has_error {
        "error".to_string()
    } else {
        "success".to_string()
    };
    let _ = (any_error, any_success); // suppress unused warnings

    run.completed_at = Some(chrono::Utc::now().to_rfc3339());

    // Persist final results
    PipelineManager::save_run(&run).await.ok();
    PipelineManager::update_last_run(&pipeline.id, &run.status)
        .await
        .ok();
    PipelineManager::prune_runs(&pipeline.id, MAX_RUNS_KEPT)
        .await
        .ok();

    info!(
        pipeline_id = %pipeline.id,
        run_id = %run.id,
        status = %run.status,
        "Pipeline run completed"
    );

    run
}

/// Result from a parallel step execution
struct StepResult {
    step_id: String,
    step_run_idx: usize,
    result: Result<(String, String, PipelineTokenUsage, u32)>,
    on_error: Option<String>,
}

/// Execute a single step in-line (used for levels with only one step)
async fn execute_single_step(
    pipeline: &PipelineDefinition,
    step_id: &str,
    config: &ResolvedConfig,
    run: &mut PipelineRun,
    step_outputs: &Arc<Mutex<HashMap<String, String>>>,
    failed_steps: &mut std::collections::HashSet<String>,
    cancel_token: &CancellationToken,
) -> bool {
    let step = pipeline.steps.iter().find(|s| s.id == step_id).unwrap();
    let step_run_idx = run
        .step_runs
        .iter()
        .position(|sr| sr.step_id == step_id)
        .unwrap();

    let outputs_snapshot = step_outputs.lock().await.clone();

    // Check conditions
    if !should_execute_step(pipeline, step_id, &outputs_snapshot, &run.step_runs) {
        run.step_runs[step_run_idx].status = "skipped".to_string();
        debug!(step_id, "Step skipped (conditions not met)");
        return false;
    }

    // Check upstream failures
    if has_failed_upstream(pipeline, step_id, failed_steps) {
        run.step_runs[step_run_idx].status = "skipped".to_string();
        debug!(step_id, "Step skipped (upstream failure)");
        return false;
    }

    let input = gather_step_input(pipeline, step_id, &outputs_snapshot);

    // ── Approval gate step ──
    if step.step_type == "approval" {
        return execute_approval_step(
            step,
            step_run_idx,
            &input,
            &outputs_snapshot,
            run,
            step_outputs,
            cancel_token,
        )
        .await;
    }

    let rendered_prompt = render_prompt(step, &input, &outputs_snapshot);

    run.step_runs[step_run_idx].status = "running".to_string();
    run.step_runs[step_run_idx].input = input.clone();
    run.step_runs[step_run_idx].started_at = Some(chrono::Utc::now().to_rfc3339());
    PipelineManager::save_run(run).await.ok();

    // Live progress: shared state updated by execute_step, saved periodically
    let progress = Arc::new(Mutex::new(StepProgress::default()));
    let progress_clone = Arc::clone(&progress);
    let step_id_owned = step_id.to_string();
    let save_cancel = CancellationToken::new();
    let save_cancel_clone = save_cancel.clone();

    // Background task: save partial progress every 2s
    let mut run_snapshot = run.clone();
    let progress_saver = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        interval.tick().await; // skip first
        loop {
            tokio::select! {
                _ = save_cancel_clone.cancelled() => break,
                _ = interval.tick() => {
                    let p = progress_clone.lock().await;
                    if let Some(sr) = run_snapshot.step_runs.iter_mut().find(|s| s.step_id == step_id_owned) {
                        sr.output = p.output.clone();
                        sr.token_usage = p.token_usage.clone();
                        sr.turns = p.turns;
                        if !p.activity.is_empty() {
                            sr.error = Some(format!("[activity] {}", p.activity));
                        }
                    }
                    PipelineManager::save_run(&run_snapshot).await.ok();
                }
            }
        }
    });

    let step_result = execute_step_with_retry_live(
        step,
        &rendered_prompt,
        config,
        Some(cancel_token),
        Some(Arc::clone(&progress)),
    )
    .await;

    // Stop the progress saver
    save_cancel.cancel();
    progress_saver.await.ok();

    match step_result {
        Ok((clean_output, display_output, token_usage, turns)) => {
            run.step_runs[step_run_idx].status = "success".to_string();
            run.step_runs[step_run_idx].output = display_output; // UI sees tool log
            run.step_runs[step_run_idx].token_usage = token_usage.clone();
            run.step_runs[step_run_idx].turns = turns;
            run.step_runs[step_run_idx].completed_at = Some(chrono::Utc::now().to_rfc3339());
            run.step_runs[step_run_idx].error = None; // clear activity

            run.token_usage.input += token_usage.input;
            run.token_usage.output += token_usage.output;
            run.token_usage.cache_creation += token_usage.cache_creation;
            run.token_usage.cache_read += token_usage.cache_read;

            // Clean output (without tool log) goes to next step
            step_outputs
                .lock()
                .await
                .insert(step_id.to_string(), clean_output);
            false
        }
        Err(e) => {
            let error_msg = format!("{e}");
            run.step_runs[step_run_idx].status = "error".to_string();
            run.step_runs[step_run_idx].error = Some(error_msg.clone());
            run.step_runs[step_run_idx].completed_at = Some(chrono::Utc::now().to_rfc3339());
            // Include partial output if any
            let p = progress.lock().await;
            if !p.output.is_empty() {
                run.step_runs[step_run_idx].output = p.output.clone();
            }

            let on_error = step.on_error.as_deref().unwrap_or("stop");
            if on_error == "skip" {
                debug!(step_id, "Step error (skip): {}", error_msg);
                step_outputs
                    .lock()
                    .await
                    .insert(step_id.to_string(), String::new());
                false
            } else {
                failed_steps.insert(step_id.to_string());
                true
            }
        }
    }
}

/// Execute an approval step within a parallel JoinSet (returns Result like agent steps)
async fn execute_approval_step_parallel(
    step: &lukan_core::pipelines::PipelineStep,
    input: &Option<String>,
    outputs_snapshot: &HashMap<String, String>,
    step_id: &str,
    shared_run: &Arc<Mutex<PipelineRun>>,
    cancel_token: &CancellationToken,
) -> Result<(String, String, PipelineTokenUsage, u32)> {
    let approval_config = step.approval.as_ref();
    let timeout_secs = approval_config.and_then(|c| c.timeout_secs).unwrap_or(3600);

    let message_template = approval_config
        .and_then(|c| c.message.as_deref())
        .unwrap_or(&step.prompt);
    let mut message = message_template.to_string();
    if let Some(inp) = input {
        message = message.replace("{{input}}", inp);
    }
    for (id, output) in outputs_snapshot {
        let placeholder = format!("{{{{prev.{id}.output}}}}");
        message = message.replace(&placeholder, output);
    }

    let approval_id = generate_run_id();
    let now = chrono::Utc::now();
    let timeout_at = now + chrono::Duration::seconds(timeout_secs as i64);

    let req = ApprovalRequest {
        id: approval_id.clone(),
        pipeline_id: shared_run.lock().await.pipeline_id.clone(),
        run_id: shared_run.lock().await.id.clone(),
        step_id: step_id.to_string(),
        context: message.clone(),
        status: "pending".to_string(),
        resolved_by: None,
        comment: None,
        created_at: now.to_rfc3339(),
        timeout_at: timeout_at.to_rfc3339(),
        resolved_at: None,
        notify_plugin: approval_config.and_then(|c| c.notify_plugin.clone()),
        notify_channel: approval_config.and_then(|c| c.notify_channel.clone()),
    };

    ApprovalManager::create(req).await?;

    // Update shared run state
    {
        let mut r = shared_run.lock().await;
        if let Some(sr) = r.step_runs.iter_mut().find(|s| s.step_id == step_id) {
            sr.status = "waiting_approval".to_string();
            sr.output = message.clone();
            sr.started_at = Some(now.to_rfc3339());
            sr.approval_id = Some(approval_id.clone());
        }
        PipelineManager::save_run(&r).await.ok();
    }

    // Send notification to plugin if configured
    if let Some(ref cfg) = step.approval
        && let (Some(plugin), Some(channel)) = (&cfg.notify_plugin, &cfg.notify_channel)
    {
        send_plugin_notification(plugin, channel, &message).await;
    }

    let poll_interval = std::time::Duration::from_secs(2);
    let timeout_deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                return Err(anyhow::anyhow!("Pipeline cancelled"));
            }
            _ = tokio::time::sleep(poll_interval) => {
                if tokio::time::Instant::now() >= timeout_deadline {
                    ApprovalManager::resolve(&approval_id, false, "timeout", Some("Approval timed out".to_string())).await.ok();
                    return Err(anyhow::anyhow!("Approval timed out after {timeout_secs}s"));
                }
                match ApprovalManager::get(&approval_id).await {
                    Ok(Some(approval)) if approval.status == "approved" => {
                        let output = approval.comment.unwrap_or_else(|| "Approved".to_string());
                        return Ok((output.clone(), output, PipelineTokenUsage::default(), 0));
                    }
                    Ok(Some(approval)) if approval.status == "rejected" => {
                        let reason = approval.comment.unwrap_or_else(|| "Rejected".to_string());
                        return Err(anyhow::anyhow!("Approval rejected: {reason}"));
                    }
                    Ok(None) => {
                        return Err(anyhow::anyhow!("Approval request file missing"));
                    }
                    _ => {} // still pending or read error, continue polling
                }
            }
        }
    }
}

/// Execute an approval gate step: create request, poll for resolution
async fn execute_approval_step(
    step: &lukan_core::pipelines::PipelineStep,
    step_run_idx: usize,
    input: &Option<String>,
    outputs_snapshot: &HashMap<String, String>,
    run: &mut PipelineRun,
    step_outputs: &Arc<Mutex<HashMap<String, String>>>,
    cancel_token: &CancellationToken,
) -> bool {
    let approval_config = step.approval.as_ref();
    let timeout_secs = approval_config.and_then(|c| c.timeout_secs).unwrap_or(3600); // default 1 hour

    // Render the approval message
    let message_template = approval_config
        .and_then(|c| c.message.as_deref())
        .unwrap_or(&step.prompt);
    let mut message = message_template.to_string();
    if let Some(inp) = input {
        message = message.replace("{{input}}", inp);
    }
    for (id, output) in outputs_snapshot {
        let placeholder = format!("{{{{prev.{id}.output}}}}");
        message = message.replace(&placeholder, output);
    }

    // Create approval request
    let approval_id = generate_run_id();
    let now = chrono::Utc::now();
    let timeout_at = now + chrono::Duration::seconds(timeout_secs as i64);

    let req = ApprovalRequest {
        id: approval_id.clone(),
        pipeline_id: run.pipeline_id.clone(),
        run_id: run.id.clone(),
        step_id: step.id.clone(),
        context: message.clone(),
        status: "pending".to_string(),
        resolved_by: None,
        comment: None,
        created_at: now.to_rfc3339(),
        timeout_at: timeout_at.to_rfc3339(),
        resolved_at: None,
        notify_plugin: approval_config.and_then(|c| c.notify_plugin.clone()),
        notify_channel: approval_config.and_then(|c| c.notify_channel.clone()),
    };

    if let Err(e) = ApprovalManager::create(req).await {
        error!(step_id = %step.id, error = %e, "Failed to create approval request");
        run.step_runs[step_run_idx].status = "error".to_string();
        run.step_runs[step_run_idx].error = Some(format!("Failed to create approval: {e}"));
        run.step_runs[step_run_idx].completed_at = Some(chrono::Utc::now().to_rfc3339());
        return true;
    }

    // Update step run status
    run.step_runs[step_run_idx].status = "waiting_approval".to_string();
    run.step_runs[step_run_idx].input = input.clone();
    run.step_runs[step_run_idx].output = message.clone();
    run.step_runs[step_run_idx].started_at = Some(now.to_rfc3339());
    run.step_runs[step_run_idx].approval_id = Some(approval_id.clone());
    PipelineManager::save_run(run).await.ok();

    info!(
        step_id = %step.id,
        approval_id = %approval_id,
        timeout_secs,
        "Approval gate waiting for human response"
    );

    // Send notification to plugin if configured
    if let Some(ref cfg) = step.approval
        && let (Some(plugin), Some(channel)) = (&cfg.notify_plugin, &cfg.notify_channel)
    {
        send_plugin_notification(plugin, channel, &message).await;
    }

    // Polling loop: check approval status every 2s
    let poll_interval = std::time::Duration::from_secs(2);
    let timeout_deadline =
        tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    loop {
        tokio::select! {
            _ = cancel_token.cancelled() => {
                run.step_runs[step_run_idx].status = "error".to_string();
                run.step_runs[step_run_idx].error = Some("Pipeline cancelled".to_string());
                run.step_runs[step_run_idx].completed_at = Some(chrono::Utc::now().to_rfc3339());
                PipelineManager::save_run(run).await.ok();
                return true;
            }
            _ = tokio::time::sleep(poll_interval) => {
                // Check timeout
                if tokio::time::Instant::now() >= timeout_deadline {
                    // Mark as timed out
                    ApprovalManager::resolve(&approval_id, false, "timeout", Some("Approval timed out".to_string())).await.ok();
                    run.step_runs[step_run_idx].status = "error".to_string();
                    run.step_runs[step_run_idx].error = Some(format!("Approval timed out after {timeout_secs}s"));
                    run.step_runs[step_run_idx].completed_at = Some(chrono::Utc::now().to_rfc3339());
                    PipelineManager::save_run(run).await.ok();
                    return true;
                }

                // Poll approval file
                match ApprovalManager::get(&approval_id).await {
                    Ok(Some(approval)) => {
                        match approval.status.as_str() {
                            "approved" => {
                                let output = approval.comment.unwrap_or_else(|| "Approved".to_string());
                                run.step_runs[step_run_idx].status = "success".to_string();
                                run.step_runs[step_run_idx].output = output.clone();
                                run.step_runs[step_run_idx].error = None;
                                run.step_runs[step_run_idx].completed_at = Some(chrono::Utc::now().to_rfc3339());
                                PipelineManager::save_run(run).await.ok();
                                step_outputs.lock().await.insert(step.id.clone(), output);
                                info!(step_id = %step.id, "Approval granted, continuing pipeline");
                                return false;
                            }
                            "rejected" => {
                                let reason = approval.comment.unwrap_or_else(|| "Rejected".to_string());
                                run.step_runs[step_run_idx].status = "error".to_string();
                                run.step_runs[step_run_idx].error = Some(format!("Approval rejected: {reason}"));
                                run.step_runs[step_run_idx].completed_at = Some(chrono::Utc::now().to_rfc3339());
                                PipelineManager::save_run(run).await.ok();
                                info!(step_id = %step.id, "Approval rejected, stopping pipeline");
                                return true;
                            }
                            _ => {
                                // Still pending, continue polling
                            }
                        }
                    }
                    Ok(None) => {
                        // File disappeared — treat as error
                        run.step_runs[step_run_idx].status = "error".to_string();
                        run.step_runs[step_run_idx].error = Some("Approval request file missing".to_string());
                        run.step_runs[step_run_idx].completed_at = Some(chrono::Utc::now().to_rfc3339());
                        PipelineManager::save_run(run).await.ok();
                        return true;
                    }
                    Err(e) => {
                        debug!(error = %e, "Error reading approval file, will retry");
                    }
                }
            }
        }
    }
}

/// Execute a step with retry logic and live progress
async fn execute_step_with_retry_live(
    step: &lukan_core::pipelines::PipelineStep,
    prompt: &str,
    config: &ResolvedConfig,
    cancel_token: Option<&CancellationToken>,
    progress: Option<Arc<Mutex<StepProgress>>>,
) -> Result<(String, String, PipelineTokenUsage, u32)> {
    match execute_step_live(step, prompt, config, cancel_token, progress.clone()).await {
        Ok(result) => Ok(result),
        Err(e) => {
            if cancel_token.map(|t| t.is_cancelled()).unwrap_or(false) {
                return Err(e);
            }
            let on_error = step.on_error.as_deref().unwrap_or("stop");
            if on_error.starts_with("retry:") {
                let max_retries: u32 = on_error
                    .strip_prefix("retry:")
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1);

                for attempt in 1..=max_retries {
                    if cancel_token.map(|t| t.is_cancelled()).unwrap_or(false) {
                        return Err(anyhow::anyhow!("Step cancelled"));
                    }
                    debug!(
                        step_id = step.id.as_str(),
                        attempt, max_retries, "Retrying step"
                    );
                    match execute_step_live(step, prompt, config, cancel_token, progress.clone())
                        .await
                    {
                        Ok(result) => return Ok(result),
                        Err(retry_err) => {
                            debug!(
                                step_id = step.id.as_str(),
                                attempt,
                                error = %retry_err,
                                "Retry failed"
                            );
                        }
                    }
                }
            }
            Err(e)
        }
    }
}

/// Check if a step has any failed upstream dependency
fn has_failed_upstream(
    pipeline: &PipelineDefinition,
    step_id: &str,
    failed_steps: &std::collections::HashSet<String>,
) -> bool {
    pipeline
        .connections
        .iter()
        .filter(|c| c.to_step == step_id && c.from_step != "__trigger__")
        .any(|c| failed_steps.contains(&c.from_step))
}

/// Live progress for a running step
#[derive(Clone, Default)]
pub struct StepProgress {
    pub output: String,
    pub activity: String, // current tool/action description
    pub token_usage: PipelineTokenUsage,
    pub turns: u32,
}

/// Execute a single step using AgentLoop, with optional live progress reporting
async fn execute_step_live(
    step: &lukan_core::pipelines::PipelineStep,
    prompt: &str,
    base_config: &ResolvedConfig,
    cancel_token: Option<&CancellationToken>,
    progress: Option<Arc<Mutex<StepProgress>>>,
) -> Result<(String, String, PipelineTokenUsage, u32)> {
    // Build config with overrides
    let mut config = base_config.clone();
    if let Some(ref p) = step.provider {
        match serde_json::from_value::<lukan_core::config::types::ProviderName>(
            serde_json::Value::String(p.clone()),
        ) {
            Ok(pn) => {
                info!(step_id = %step.id, provider = %p, "Step using custom provider");
                config.config.provider = pn;
            }
            Err(e) => {
                error!(step_id = %step.id, provider = %p, error = %e, "Invalid provider name, using default");
            }
        }
    }
    if let Some(ref m) = step.model {
        info!(step_id = %step.id, model = %m, "Step using custom model");
        config.config.model = Some(m.clone());
    }

    // Create provider
    let provider = create_provider(&config)?;

    // Build tool registry
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project_cfg = lukan_core::config::ProjectConfig::load(&cwd)
        .await
        .ok()
        .flatten()
        .map(|(_, cfg)| cfg);

    let permissions = project_cfg
        .as_ref()
        .map(|c| c.permissions.clone())
        .unwrap_or_default();

    let allowed = project_cfg
        .as_ref()
        .map(|c| c.resolve_allowed_paths(&cwd))
        .unwrap_or_else(|| vec![cwd.clone()]);

    let mut registry = create_configured_registry(&permissions, &allowed);
    if let Some(ref tool_names) = step.tools {
        let refs: Vec<&str> = tool_names.iter().map(|s| s.as_str()).collect();
        registry.retain(&refs);
    }

    // Propagate tool restrictions to sub-agents so they can't bypass them
    crate::sub_agent::set_tool_filter(step.tools.clone()).await;

    let provider_name = config.config.provider.to_string();
    let model_name = config.effective_model().unwrap_or_default();

    let system_prompt = SystemPrompt::Text(
        "You are a pipeline step agent. Execute the task directly using the simplest approach. \
         Do NOT use sub-agents or explore agents. Use tools directly. \
         Be concise — complete the task and report the result in minimal text."
            .to_string(),
    );

    let agent_config = AgentConfig {
        provider: Arc::from(provider),
        tools: registry,
        system_prompt,
        cwd,
        provider_name,
        model_name,
        bg_signal: None,
        allowed_paths: Some(allowed),
        permission_mode: lukan_core::config::types::PermissionMode::Skip,
        permission_mode_rx: None,
        permissions,
        approval_rx: None,
        plan_review_rx: None,
        planner_answer_rx: None,
        browser_tools: false,
        skip_session_save: true,
        vision_provider: None,
        extra_env: config.credentials.flatten_skill_env(),
    };

    let mut agent = AgentLoop::new(agent_config).await?;
    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);

    let prompt_owned = prompt.to_string();
    let max_turns = step.max_turns.unwrap_or(10);
    let timeout_secs = step.timeout_secs.unwrap_or(120); // default 2 min

    let agent_handle = tokio::spawn(async move {
        let result = agent.run_turn(&prompt_owned, event_tx, None, None).await;
        (agent, result)
    });

    let mut output = String::new();
    let mut token_usage = PipelineTokenUsage::default();
    let mut turns: u32 = 0;

    let timeout_duration = std::time::Duration::from_secs(timeout_secs);
    let cancel = cancel_token.cloned();

    // Track tool calls for the activity log
    let mut tool_log = String::new();

    let event_loop = async {
        let mut last_progress_update = std::time::Instant::now();
        let progress_interval = std::time::Duration::from_secs(1);

        loop {
            let event = if let Some(ref ct) = cancel {
                tokio::select! {
                    _ = ct.cancelled() => return Err("cancelled"),
                    ev = event_rx.recv() => ev,
                }
            } else {
                event_rx.recv().await
            };

            let Some(event) = event else { break };

            match &event {
                StreamEvent::TextDelta { text } => {
                    output.push_str(text);
                }
                StreamEvent::ToolUseStart { name, .. } => {
                    tool_log.push_str(&format!("→ {name}\n"));
                }
                StreamEvent::ToolResult { name, is_error, .. } => {
                    if *is_error == Some(true) {
                        tool_log.push_str(&format!("  ✗ {name} failed\n"));
                    }
                }
                StreamEvent::Usage {
                    input_tokens,
                    output_tokens,
                    cache_creation_tokens,
                    cache_read_tokens,
                } => {
                    token_usage.input += input_tokens;
                    token_usage.output += output_tokens;
                    if let Some(cc) = cache_creation_tokens {
                        token_usage.cache_creation += cc;
                    }
                    if let Some(cr) = cache_read_tokens {
                        token_usage.cache_read += cr;
                    }
                }
                StreamEvent::MessageEnd { .. } => {
                    turns += 1;
                    if turns >= max_turns {
                        break;
                    }
                }
                _ => {}
            }

            // Update live progress
            if let Some(ref prog) = progress {
                // Always update activity on tool events
                let activity = match &event {
                    StreamEvent::ToolUseStart { name, .. } => Some(format!("calling {name}...")),
                    StreamEvent::ToolResult { name, is_error, .. } => {
                        if *is_error == Some(true) {
                            Some(format!("{name} failed"))
                        } else {
                            Some(format!("{name} done"))
                        }
                    }
                    _ => None,
                };

                let should_update =
                    activity.is_some() || last_progress_update.elapsed() >= progress_interval;

                if should_update {
                    let mut p = prog.lock().await;
                    p.output = output.clone();
                    p.token_usage = token_usage.clone();
                    p.turns = turns;
                    if let Some(act) = activity {
                        p.activity = act;
                    }
                    last_progress_update = std::time::Instant::now();
                }
            }
        }
        Ok(())
    };

    let result = tokio::time::timeout(timeout_duration, event_loop).await;

    // Always abort the agent task first
    agent_handle.abort();

    // Build display output (with tool log for UI) separate from clean output (for next step)
    let display_output = if tool_log.is_empty() {
        output.clone()
    } else {
        format!("[tools used]\n{tool_log}\n[output]\n{output}")
    };

    match result {
        Err(_) => {
            // Timed out
            if output.is_empty() {
                return Err(anyhow::anyhow!(
                    "Step timed out after {timeout_secs}s without producing output.\nTool activity:\n{tool_log}"
                ));
            }
            // output = clean (for next step), display_output = with tool log (for UI)
            Ok((output, display_output, token_usage, turns))
        }
        Ok(Err("cancelled")) => {
            if output.is_empty() {
                Err(anyhow::anyhow!(
                    "Step cancelled.\nTool activity:\n{tool_log}"
                ))
            } else {
                Ok((output, display_output, token_usage, turns))
            }
        }
        Ok(_) => {
            if output.is_empty() {
                return Err(anyhow::anyhow!(
                    "Step completed {turns} turns without producing text output.\nTool activity:\n{tool_log}"
                ));
            }
            Ok((output, display_output, token_usage, turns))
        }
    }
}

/// Topological sort into levels — steps in the same level can run in parallel
fn topological_levels(pipeline: &PipelineDefinition) -> Result<Vec<Vec<String>>> {
    let step_ids: Vec<String> = pipeline.steps.iter().map(|s| s.id.clone()).collect();
    let mut in_degree: HashMap<String, usize> = HashMap::new();
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();

    for id in &step_ids {
        in_degree.insert(id.clone(), 0);
        adjacency.insert(id.clone(), Vec::new());
    }

    for conn in &pipeline.connections {
        if conn.from_step == "__trigger__" {
            continue;
        }
        if let Some(deg) = in_degree.get_mut(&conn.to_step) {
            *deg += 1;
        }
        if let Some(adj) = adjacency.get_mut(&conn.from_step) {
            adj.push(conn.to_step.clone());
        }
    }

    let mut levels: Vec<Vec<String>> = Vec::new();
    let mut current_level: Vec<String> = step_ids
        .iter()
        .filter(|id| in_degree.get(*id).copied().unwrap_or(0) == 0)
        .cloned()
        .collect();

    let mut visited = 0;

    while !current_level.is_empty() {
        visited += current_level.len();
        let mut next_level = Vec::new();

        for id in &current_level {
            if let Some(neighbors) = adjacency.get(id) {
                for neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            next_level.push(neighbor.clone());
                        }
                    }
                }
            }
        }

        levels.push(std::mem::take(&mut current_level));
        current_level = next_level;
    }

    if visited != step_ids.len() {
        return Err(anyhow::anyhow!("Pipeline has circular dependencies"));
    }

    Ok(levels)
}

/// Determine if a step should execute based on its incoming connections' conditions
fn should_execute_step(
    pipeline: &PipelineDefinition,
    step_id: &str,
    step_outputs: &HashMap<String, String>,
    step_runs: &[StepRun],
) -> bool {
    let incoming: Vec<_> = pipeline
        .connections
        .iter()
        .filter(|c| c.to_step == step_id)
        .collect();

    // If no incoming connections, this is a root step — always execute
    if incoming.is_empty() {
        return true;
    }

    // At least one incoming connection must be satisfied
    incoming.iter().any(|conn| {
        let from_output = step_outputs.get(&conn.from_step);
        let from_status = step_runs
            .iter()
            .find(|sr| sr.step_id == conn.from_step)
            .map(|sr| sr.status.as_str());

        match &conn.condition {
            None | Some(StepCondition::Always) => {
                // Source must have produced output (executed successfully or at least ran)
                from_output.is_some() || conn.from_step == "__trigger__"
            }
            Some(StepCondition::Contains { value }) => from_output
                .map(|o| o.contains(value.as_str()))
                .unwrap_or(false),
            Some(StepCondition::Matches { pattern }) => from_output
                .map(|o| {
                    regex::Regex::new(pattern)
                        .map(|re| re.is_match(o))
                        .unwrap_or(false)
                })
                .unwrap_or(false),
            Some(StepCondition::Status { status }) => {
                from_status.map(|s| s == status.as_str()).unwrap_or(false)
            }
        }
    })
}

/// Gather input for a step from upstream step outputs
fn gather_step_input(
    pipeline: &PipelineDefinition,
    step_id: &str,
    step_outputs: &HashMap<String, String>,
) -> Option<String> {
    let incoming: Vec<_> = pipeline
        .connections
        .iter()
        .filter(|c| c.to_step == step_id)
        .collect();

    if incoming.is_empty() {
        return None;
    }

    let mut inputs = Vec::new();
    for conn in &incoming {
        if let Some(output) = step_outputs.get(&conn.from_step) {
            inputs.push(output.clone());
        }
    }

    if inputs.is_empty() {
        None
    } else if inputs.len() == 1 {
        Some(inputs.into_iter().next().unwrap())
    } else {
        Some(inputs.join("\n\n---\n\n"))
    }
}

/// Render the step prompt with template variables
fn render_prompt(
    step: &lukan_core::pipelines::PipelineStep,
    input: &Option<String>,
    step_outputs: &HashMap<String, String>,
) -> String {
    let template = step.prompt_template.as_deref().unwrap_or(&step.prompt);

    let mut result = template.to_string();

    // Replace {{input}} with the gathered input
    if let Some(inp) = input {
        result = result.replace("{{input}}", inp);
    } else {
        result = result.replace("{{input}}", "");
    }

    // Replace {{prev.step_id.output}} patterns
    for (id, output) in step_outputs {
        let placeholder = format!("{{{{prev.{id}.output}}}}");
        result = result.replace(&placeholder, output);
    }

    // If the template is just the prompt and there's input, append it
    if step.prompt_template.is_none()
        && let Some(inp) = input
        && !inp.is_empty()
    {
        result = format!("{result}\n\nInput:\n{inp}");
    }

    result
}

/// Send a notification message to a plugin's channel.
/// For WhatsApp/Telegram/etc: reads the plugin's config.json to find the connector URL,
/// then sends a message directly via WebSocket.
async fn send_plugin_notification(plugin: &str, channel_id: &str, text: &str) {
    // Read plugin config to get the connector URL
    let config_path = lukan_core::config::LukanPaths::plugin_config(plugin);
    let bridge_url = if let Ok(data) = tokio::fs::read_to_string(&config_path).await {
        serde_json::from_str::<serde_json::Value>(&data)
            .ok()
            .and_then(|v| {
                v.get("bridgeUrl")
                    .and_then(|u| u.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| "ws://localhost:3001".to_string())
    } else {
        "ws://localhost:3001".to_string()
    };

    // Truncate text for notification (WhatsApp messages can be long)
    let notify_text = if text.len() > 2000 {
        format!(
            "{}...\n\n[Respond 'yes' to approve or 'no' to reject]",
            &text[..2000]
        )
    } else {
        format!("{}\n\n[Respond 'yes' to approve or 'no' to reject]", text)
    };

    info!(plugin, channel_id, bridge_url = %bridge_url, "Sending approval notification");

    // Connect to the connector WebSocket and send the message
    match tokio_tungstenite::connect_async(&bridge_url).await {
        Ok((mut ws, _)) => {
            let msg = serde_json::json!({
                "type": "send",
                "to": channel_id,
                "text": notify_text,
            });
            use futures::SinkExt;
            if let Err(e) = ws
                .send(tokio_tungstenite::tungstenite::Message::Text(
                    msg.to_string().into(),
                ))
                .await
            {
                error!(plugin, error = %e, "Failed to send notification via WebSocket");
            } else {
                info!(
                    plugin,
                    channel_id, "Approval notification sent successfully"
                );
                // Wait for the connector to process the message before closing
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
            ws.close(None).await.ok();
        }
        Err(e) => {
            error!(
                plugin,
                bridge_url = %bridge_url,
                error = %e,
                "Failed to connect to plugin connector for notification"
            );
        }
    }
}

fn generate_run_id() -> String {
    let bytes: [u8; 3] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
