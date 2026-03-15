use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use rand::Rng;
use tokio::sync::{Mutex, mpsc};
use tokio::task::JoinSet;
use tracing::{debug, error, info};

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
            })
            .collect(),
        trigger_input: trigger_input.clone(),
        token_usage: PipelineTokenUsage::default(),
    };

    // Save initial "running" state
    if let Err(e) = PipelineManager::save_run(&run).await {
        error!(error = %e, "Failed to save initial pipeline run");
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
    let step_outputs: Arc<Mutex<HashMap<String, String>>> =
        Arc::new(Mutex::new(HashMap::new()));

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

    // Execute level by level
    for level in &levels {
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
                    debug!(step_id = step_id.as_str(), "Step skipped (conditions not met)");
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
                let rendered_prompt = render_prompt(&step, &input, &outputs_snapshot);

                run.step_runs[step_run_idx].status = "running".to_string();
                run.step_runs[step_run_idx].input = input.clone();
                run.step_runs[step_run_idx].started_at = Some(chrono::Utc::now().to_rfc3339());

                let config_clone = config.clone();
                let sid = step_id.clone();

                join_set.spawn(async move {
                    let result = execute_step_with_retry(&step, &rendered_prompt, &config_clone).await;
                    StepResult {
                        step_id: sid,
                        step_run_idx,
                        result,
                        on_error: step.on_error.clone(),
                    }
                });
            }

            // Save progress (all steps in level are now "running")
            PipelineManager::save_run(&run).await.ok();

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
                    Ok((output, token_usage, turns)) => {
                        run.step_runs[sr.step_run_idx].status = "success".to_string();
                        run.step_runs[sr.step_run_idx].output = output.clone();
                        run.step_runs[sr.step_run_idx].token_usage = token_usage.clone();
                        run.step_runs[sr.step_run_idx].turns = turns;
                        run.step_runs[sr.step_run_idx].completed_at =
                            Some(chrono::Utc::now().to_rfc3339());

                        run.token_usage.input += token_usage.input;
                        run.token_usage.output += token_usage.output;
                        run.token_usage.cache_creation += token_usage.cache_creation;
                        run.token_usage.cache_read += token_usage.cache_read;

                        step_outputs
                            .lock()
                            .await
                            .insert(sr.step_id.clone(), output);
                    }
                    Err(e) => {
                        let error_msg = format!("{e}");
                        run.step_runs[sr.step_run_idx].status = "error".to_string();
                        run.step_runs[sr.step_run_idx].error = Some(error_msg.clone());
                        run.step_runs[sr.step_run_idx].completed_at =
                            Some(chrono::Utc::now().to_rfc3339());

                        let on_error = sr.on_error.as_deref().unwrap_or("stop");
                        if on_error == "skip" {
                            debug!(step_id = sr.step_id.as_str(), "Step error (skip): {}", error_msg);
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
    result: Result<(String, PipelineTokenUsage, u32)>,
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
    let rendered_prompt = render_prompt(step, &input, &outputs_snapshot);

    run.step_runs[step_run_idx].status = "running".to_string();
    run.step_runs[step_run_idx].input = input.clone();
    run.step_runs[step_run_idx].started_at = Some(chrono::Utc::now().to_rfc3339());
    PipelineManager::save_run(run).await.ok();

    let step_result = execute_step_with_retry(step, &rendered_prompt, config).await;

    match step_result {
        Ok((output, token_usage, turns)) => {
            run.step_runs[step_run_idx].status = "success".to_string();
            run.step_runs[step_run_idx].output = output.clone();
            run.step_runs[step_run_idx].token_usage = token_usage.clone();
            run.step_runs[step_run_idx].turns = turns;
            run.step_runs[step_run_idx].completed_at = Some(chrono::Utc::now().to_rfc3339());

            run.token_usage.input += token_usage.input;
            run.token_usage.output += token_usage.output;
            run.token_usage.cache_creation += token_usage.cache_creation;
            run.token_usage.cache_read += token_usage.cache_read;

            step_outputs
                .lock()
                .await
                .insert(step_id.to_string(), output);
            false
        }
        Err(e) => {
            let error_msg = format!("{e}");
            run.step_runs[step_run_idx].status = "error".to_string();
            run.step_runs[step_run_idx].error = Some(error_msg.clone());
            run.step_runs[step_run_idx].completed_at = Some(chrono::Utc::now().to_rfc3339());

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

/// Execute a step with retry logic
async fn execute_step_with_retry(
    step: &lukan_core::pipelines::PipelineStep,
    prompt: &str,
    config: &ResolvedConfig,
) -> Result<(String, PipelineTokenUsage, u32)> {
    match execute_step(step, prompt, config).await {
        Ok(result) => Ok(result),
        Err(e) => {
            let on_error = step.on_error.as_deref().unwrap_or("stop");
            if on_error.starts_with("retry:") {
                let max_retries: u32 = on_error
                    .strip_prefix("retry:")
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1);

                for attempt in 1..=max_retries {
                    debug!(step_id = step.id.as_str(), attempt, max_retries, "Retrying step");
                    match execute_step(step, prompt, config).await {
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

/// Execute a single step using AgentLoop
async fn execute_step(
    step: &lukan_core::pipelines::PipelineStep,
    prompt: &str,
    base_config: &ResolvedConfig,
) -> Result<(String, PipelineTokenUsage, u32)> {
    // Build config with overrides
    let mut config = base_config.clone();
    if let Some(ref p) = step.provider
        && let Ok(pn) = serde_json::from_value(serde_json::Value::String(p.clone()))
    {
        config.config.provider = pn;
    }
    if let Some(ref m) = step.model {
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

    let provider_name = config.config.provider.to_string();
    let model_name = config.effective_model().unwrap_or_default();

    let system_prompt = SystemPrompt::Text(
        "You are a pipeline step agent. Execute the task described in the user message. \
         Be concise and focused. Complete the task and report results."
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

    let agent_handle = tokio::spawn(async move {
        let result = agent.run_turn(&prompt_owned, event_tx, None, None).await;
        (agent, result)
    });

    let mut output = String::new();
    let mut token_usage = PipelineTokenUsage::default();
    let mut turns: u32 = 0;

    while let Some(event) = event_rx.recv().await {
        match &event {
            StreamEvent::TextDelta { text } => {
                output.push_str(text);
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
    }

    match agent_handle.await {
        Ok((_agent, result)) => {
            if let Err(e) = result {
                return Err(anyhow::anyhow!("{e}"));
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!("Agent task panicked: {e}"));
        }
    }

    Ok((output, token_usage, turns))
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
                .map(|o| regex::Regex::new(pattern).map(|re| re.is_match(o)).unwrap_or(false))
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
    let template = step
        .prompt_template
        .as_deref()
        .unwrap_or(&step.prompt);

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
    if step.prompt_template.is_none() {
        if let Some(inp) = input {
            if !inp.is_empty() {
                result = format!("{result}\n\nInput:\n{inp}");
            }
        }
    }

    result
}

fn generate_run_id() -> String {
    let bytes: [u8; 3] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
