use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, broadcast};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use lukan_core::config::ResolvedConfig;
use lukan_core::pipelines::{
    PipelineCreateInput, PipelineDefinition, PipelineDetail, PipelineManager, PipelineRun,
    PipelineSummary, PipelineTrigger, PipelineUpdateInput,
};

use super::executor::execute_pipeline_full;

/// Notification emitted when a pipeline run completes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineNotification {
    pub pipeline_id: String,
    pub pipeline_name: String,
    pub status: String,
    pub summary: String,
}

/// Key fields used to detect when a pipeline needs rescheduling
#[derive(Clone, PartialEq)]
struct PipelineSnapshot {
    enabled: bool,
    trigger: String, // serialized trigger for comparison
    steps_hash: String,
}

impl From<&PipelineDefinition> for PipelineSnapshot {
    fn from(p: &PipelineDefinition) -> Self {
        Self {
            enabled: p.enabled,
            trigger: serde_json::to_string(&p.trigger).unwrap_or_default(),
            steps_hash: format!("{}-{}", p.steps.len(), p.connections.len()),
        }
    }
}

/// Scheduler that manages pipeline timers and execution
pub struct PipelineScheduler {
    config: Mutex<ResolvedConfig>,
    timers: Mutex<HashMap<String, JoinHandle<()>>>,
    running_pipelines: Arc<Mutex<HashSet<String>>>,
    /// Per-run cancellation tokens: keyed by pipeline_id
    run_cancel_tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
    cancel_token: CancellationToken,
    notify_tx: broadcast::Sender<PipelineNotification>,
    started: AtomicBool,
    known_state: Mutex<HashMap<String, PipelineSnapshot>>,
}

impl PipelineScheduler {
    pub fn new(config: ResolvedConfig) -> Self {
        let (notify_tx, _) = broadcast::channel(64);
        Self {
            config: Mutex::new(config),
            timers: Mutex::new(HashMap::new()),
            running_pipelines: Arc::new(Mutex::new(HashSet::new())),
            run_cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
            cancel_token: CancellationToken::new(),
            notify_tx,
            started: AtomicBool::new(false),
            known_state: Mutex::new(HashMap::new()),
        }
    }

    /// Subscribe to pipeline notifications (run completions)
    pub fn subscribe(&self) -> broadcast::Receiver<PipelineNotification> {
        self.notify_tx.subscribe()
    }

    /// Start the scheduler: load all enabled scheduled/event/filewatch pipelines
    pub async fn start(&self) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }

        info!("Starting pipeline scheduler");
        if let Err(e) = PipelineManager::cleanup_stale_runs().await {
            error!(error = %e, "Failed to cleanup stale pipeline runs");
        }
        match PipelineManager::list().await {
            Ok(pipelines) => {
                let mut known = self.known_state.lock().await;
                for pipeline in &pipelines {
                    known.insert(pipeline.id.clone(), PipelineSnapshot::from(pipeline));
                    if pipeline.enabled {
                        match &pipeline.trigger {
                            PipelineTrigger::Schedule { .. } => {
                                self.schedule_pipeline(pipeline).await;
                            }
                            PipelineTrigger::Event { .. } => {
                                self.start_event_watcher(pipeline).await;
                            }
                            PipelineTrigger::FileWatch { .. } => {
                                self.start_file_watcher(pipeline).await;
                            }
                            _ => {}
                        }
                    }
                }
                let count = self.timers.lock().await.len();
                info!(count, "Pipeline scheduler started");
            }
            Err(e) => {
                error!(error = %e, "Failed to load pipelines on scheduler start");
            }
        }
    }

    /// Reload pipelines from disk and reconcile timers.
    pub async fn reload(&self) {
        let pipelines = match PipelineManager::list().await {
            Ok(p) => p,
            Err(e) => {
                error!(error = %e, "Failed to reload pipelines");
                return;
            }
        };

        let new_map: HashMap<String, &PipelineDefinition> =
            pipelines.iter().map(|p| (p.id.clone(), p)).collect();

        let mut known = self.known_state.lock().await;
        let mut timers = self.timers.lock().await;

        // Cancel timers for removed or newly-disabled pipelines
        let timer_ids: Vec<String> = timers.keys().cloned().collect();
        for id in &timer_ids {
            let should_cancel = match new_map.get(id) {
                None => true,
                Some(p) if !p.enabled => true,
                Some(p) => !matches!(
                    p.trigger,
                    PipelineTrigger::Schedule { .. }
                        | PipelineTrigger::Event { .. }
                        | PipelineTrigger::FileWatch { .. }
                ),
            };
            if should_cancel && let Some(handle) = timers.remove(id) {
                handle.abort();
                debug!(pipeline_id = %id, "Cancelled pipeline timer (reload)");
            }
        }

        // Collect pipelines that need (re)scheduling
        let mut to_schedule: Vec<PipelineDefinition> = Vec::new();
        for p in &pipelines {
            let snap = PipelineSnapshot::from(p);
            let has_auto_trigger = p.enabled
                && matches!(
                    p.trigger,
                    PipelineTrigger::Schedule { .. }
                        | PipelineTrigger::Event { .. }
                        | PipelineTrigger::FileWatch { .. }
                );

            if !has_auto_trigger {
                known.insert(p.id.clone(), snap);
                continue;
            }

            let needs_schedule = match known.get(&p.id) {
                None => true,
                Some(old) => *old != snap,
            };

            if needs_schedule {
                if let Some(handle) = timers.remove(&p.id) {
                    handle.abort();
                }
                to_schedule.push(p.clone());
            }
            known.insert(p.id.clone(), snap);
        }

        known.retain(|id, _| new_map.contains_key(id));

        drop(timers);
        drop(known);

        for p in &to_schedule {
            match &p.trigger {
                PipelineTrigger::Schedule { .. } => self.schedule_pipeline(p).await,
                PipelineTrigger::Event { .. } => self.start_event_watcher(p).await,
                PipelineTrigger::FileWatch { .. } => self.start_file_watcher(p).await,
                _ => {}
            }
            debug!(pipeline_id = %p.id, "Rescheduled pipeline (reload)");
        }
    }

    /// Stop all timers
    pub fn stop(&self) {
        if !self.started.swap(false, Ordering::SeqCst) {
            return;
        }
        info!("Stopping pipeline scheduler");
        self.cancel_token.cancel();
    }

    // ── CRUD pass-throughs ──

    pub async fn list_pipelines(&self) -> Result<Vec<PipelineSummary>> {
        PipelineManager::get_summaries().await
    }

    pub async fn create_pipeline(&self, input: PipelineCreateInput) -> Result<PipelineDefinition> {
        let pipeline = PipelineManager::create(input).await?;
        if pipeline.enabled
            && let PipelineTrigger::Schedule { .. } = &pipeline.trigger
        {
            self.schedule_pipeline(&pipeline).await;
        }
        Ok(pipeline)
    }

    pub async fn update_pipeline(
        &self,
        id: &str,
        patch: PipelineUpdateInput,
    ) -> Result<Option<PipelineDefinition>> {
        let result = PipelineManager::update(id, patch).await?;
        if let Some(ref p) = result {
            self.reschedule_pipeline(p).await;
        }
        Ok(result)
    }

    pub async fn delete_pipeline(&self, id: &str) -> Result<bool> {
        self.cancel_timer(id).await;
        PipelineManager::delete(id).await
    }

    pub async fn toggle_pipeline(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<Option<PipelineDefinition>> {
        let patch = PipelineUpdateInput {
            enabled: Some(enabled),
            name: None,
            description: None,
            trigger: None,
            steps: None,
            connections: None,
        };
        let result = PipelineManager::update(id, patch).await?;
        if let Some(ref p) = result {
            self.reschedule_pipeline(p).await;
        }
        Ok(result)
    }

    pub async fn get_pipeline_detail(&self, id: &str) -> Result<Option<PipelineDetail>> {
        PipelineManager::get_detail(id).await
    }

    pub async fn get_pipeline_run_detail(
        &self,
        pipeline_id: &str,
        run_id: &str,
    ) -> Result<Option<PipelineRun>> {
        PipelineManager::get_run(pipeline_id, run_id).await
    }

    /// Cancel a running pipeline
    pub async fn cancel_pipeline(&self, id: &str) -> Result<bool> {
        let tokens = self.run_cancel_tokens.lock().await;
        if let Some(token) = tokens.get(id) {
            token.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check if a pipeline is currently running
    pub async fn is_pipeline_running(&self, id: &str) -> bool {
        self.running_pipelines.lock().await.contains(id)
    }

    /// Trigger a pipeline run manually
    pub async fn trigger_pipeline(&self, id: &str, input: Option<String>) -> Result<()> {
        let pipeline = PipelineManager::get(id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Pipeline not found: {id}"))?;

        let config = self.config.lock().await.clone();
        let running_pipelines = Arc::clone(&self.running_pipelines);
        let run_cancel_tokens = Arc::clone(&self.run_cancel_tokens);
        let notify_tx = self.notify_tx.clone();
        let pipeline_id = pipeline.id.clone();
        let pipeline_name = pipeline.name.clone();

        // Create a cancellation token for this run
        let run_token = CancellationToken::new();
        run_cancel_tokens
            .lock()
            .await
            .insert(pipeline_id.clone(), run_token.clone());

        tokio::spawn(async move {
            // Guard: skip if already running
            {
                let mut running = running_pipelines.lock().await;
                if running.contains(&pipeline_id) {
                    debug!(pipeline_id = %pipeline_id, "Pipeline already running, skipping");
                    return;
                }
                running.insert(pipeline_id.clone());
            }

            let run = execute_pipeline_full(
                &pipeline,
                input,
                &config,
                run_token,
                Some(notify_tx.clone()),
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

            let notification = PipelineNotification {
                pipeline_id: pipeline_id.clone(),
                pipeline_name,
                status: run.status,
                summary,
            };
            let _ = notify_tx.send(notification);

            running_pipelines.lock().await.remove(&pipeline_id);
            run_cancel_tokens.lock().await.remove(&pipeline_id);
        });

        Ok(())
    }

    // ── Internal scheduling ──

    async fn schedule_pipeline(&self, pipeline: &PipelineDefinition) {
        let schedule = match &pipeline.trigger {
            PipelineTrigger::Schedule { schedule } => schedule.clone(),
            _ => return,
        };

        let interval_ms = match lukan_core::workers::schedule::parse_schedule_ms(&schedule) {
            Ok(ms) => ms,
            Err(e) => {
                error!(
                    pipeline_id = %pipeline.id,
                    schedule = %schedule,
                    error = %e,
                    "Invalid schedule, not scheduling pipeline"
                );
                return;
            }
        };

        let pipeline_clone = pipeline.clone();
        let config = self.config.lock().await.clone();
        let running_pipelines = Arc::clone(&self.running_pipelines);
        let notify_tx = self.notify_tx.clone();
        let cancel_token = self.cancel_token.clone();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
            interval.tick().await; // Skip first immediate tick

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        debug!(pipeline_id = %pipeline_clone.id, "Pipeline timer cancelled");
                        break;
                    }
                    _ = interval.tick() => {
                        let pipeline_id = pipeline_clone.id.clone();
                        let pipeline_name = pipeline_clone.name.clone();

                        // Guard: skip if already running
                        {
                            let mut running = running_pipelines.lock().await;
                            if running.contains(&pipeline_id) {
                                debug!(pipeline_id = %pipeline_id, "Pipeline already running, skipping tick");
                                continue;
                            }
                            running.insert(pipeline_id.clone());
                        }

                        let run = execute_pipeline_full(
                            &pipeline_clone, None, &config,
                            tokio_util::sync::CancellationToken::new(),
                            Some(notify_tx.clone()),
                        ).await;

                        let summary = if run.status == "success" {
                            let step_count = run.step_runs.iter().filter(|s| s.status == "success").count();
                            format!("{step_count} steps completed successfully")
                        } else {
                            let error_step = run.step_runs.iter().find(|s| s.status == "error");
                            error_step
                                .and_then(|s| s.error.clone())
                                .unwrap_or_else(|| format!("Pipeline {}", run.status))
                        };

                        let notification = PipelineNotification {
                            pipeline_id: pipeline_id.clone(),
                            pipeline_name,
                            status: run.status,
                            summary,
                        };
                        let _ = notify_tx.send(notification);

                        running_pipelines.lock().await.remove(&pipeline_id);
                    }
                }
            }
        });

        let mut timers = self.timers.lock().await;
        if let Some(old) = timers.insert(pipeline.id.clone(), handle) {
            old.abort();
        }
        debug!(
            pipeline_id = %pipeline.id,
            interval_ms,
            "Scheduled pipeline"
        );
    }

    async fn reschedule_pipeline(&self, pipeline: &PipelineDefinition) {
        self.cancel_timer(&pipeline.id).await;
        if pipeline.enabled {
            match &pipeline.trigger {
                PipelineTrigger::Schedule { .. } => self.schedule_pipeline(pipeline).await,
                PipelineTrigger::Event { .. } => self.start_event_watcher(pipeline).await,
                PipelineTrigger::FileWatch { .. } => self.start_file_watcher(pipeline).await,
                _ => {}
            }
        }
    }

    async fn cancel_timer(&self, id: &str) {
        let mut timers = self.timers.lock().await;
        if let Some(handle) = timers.remove(id) {
            handle.abort();
            debug!(pipeline_id = %id, "Cancelled pipeline timer");
        }
    }

    /// Start an event watcher that polls pending.jsonl for matching events
    async fn start_event_watcher(&self, pipeline: &PipelineDefinition) {
        let (source_filter, level_filter) = match &pipeline.trigger {
            PipelineTrigger::Event { source, level } => (source.clone(), level.clone()),
            _ => return,
        };

        let pipeline_clone = pipeline.clone();
        let config = self.config.lock().await.clone();
        let running_pipelines = Arc::clone(&self.running_pipelines);
        let notify_tx = self.notify_tx.clone();
        let cancel_token = self.cancel_token.clone();
        let source_for_log = source_filter.clone();

        let handle = tokio::spawn(async move {
            let pending_path = lukan_core::config::LukanPaths::pending_events_file();
            let mut last_size: u64 = tokio::fs::metadata(&pending_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);

            let mut interval = tokio::time::interval(Duration::from_secs(2));
            interval.tick().await; // Skip first

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => break,
                    _ = interval.tick() => {
                        let current_size = tokio::fs::metadata(&pending_path)
                            .await
                            .map(|m| m.len())
                            .unwrap_or(0);

                        if current_size <= last_size {
                            last_size = current_size;
                            continue;
                        }

                        // Read new lines
                        let content = match tokio::fs::read_to_string(&pending_path).await {
                            Ok(c) => c,
                            Err(_) => continue,
                        };

                        let mut matched_event = None;
                        for line in content.lines().rev().take(50) {
                            if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                                let ev_source = event["source"].as_str().unwrap_or("");
                                let ev_level = event["level"].as_str().unwrap_or("");

                                if ev_source == source_filter {
                                    if let Some(ref lf) = level_filter
                                        && ev_level != lf
                                    {
                                        continue;
                                    }
                                    matched_event = Some(line.to_string());
                                    break;
                                }
                            }
                        }

                        last_size = current_size;

                        let Some(event_json) = matched_event else {
                            continue;
                        };

                        let pipeline_id = pipeline_clone.id.clone();
                        let pipeline_name = pipeline_clone.name.clone();

                        {
                            let mut running = running_pipelines.lock().await;
                            if running.contains(&pipeline_id) {
                                continue;
                            }
                            running.insert(pipeline_id.clone());
                        }

                        let run = execute_pipeline_full(
                            &pipeline_clone,
                            Some(event_json),
                            &config,
                            tokio_util::sync::CancellationToken::new(),
                            Some(notify_tx.clone()),
                        )
                        .await;

                        let summary = if run.status == "success" {
                            let c = run.step_runs.iter().filter(|s| s.status == "success").count();
                            format!("{c} steps completed successfully")
                        } else {
                            run.step_runs.iter().find(|s| s.status == "error")
                                .and_then(|s| s.error.clone())
                                .unwrap_or_else(|| format!("Pipeline {}", run.status))
                        };

                        let _ = notify_tx.send(PipelineNotification {
                            pipeline_id: pipeline_id.clone(),
                            pipeline_name,
                            status: run.status,
                            summary,
                        });

                        running_pipelines.lock().await.remove(&pipeline_id);
                    }
                }
            }
        });

        let mut timers = self.timers.lock().await;
        if let Some(old) = timers.insert(pipeline.id.clone(), handle) {
            old.abort();
        }
        debug!(
            pipeline_id = %pipeline.id,
            source = %source_for_log,
            "Started event watcher for pipeline"
        );
    }

    /// Start a file watcher that polls for file modifications
    async fn start_file_watcher(&self, pipeline: &PipelineDefinition) {
        let (watch_path, debounce_secs) = match &pipeline.trigger {
            PipelineTrigger::FileWatch {
                path,
                debounce_secs,
            } => (path.clone(), debounce_secs.unwrap_or(5)),
            _ => return,
        };

        let pipeline_clone = pipeline.clone();
        let config = self.config.lock().await.clone();
        let running_pipelines = Arc::clone(&self.running_pipelines);
        let notify_tx = self.notify_tx.clone();
        let cancel_token = self.cancel_token.clone();
        let watch_path_for_log = watch_path.clone();

        let handle = tokio::spawn(async move {
            let path = std::path::PathBuf::from(&watch_path);

            // Get initial mtime
            let get_mtime = |p: &std::path::Path| -> Option<std::time::SystemTime> {
                std::fs::metadata(p).ok().and_then(|m| m.modified().ok())
            };

            let mut last_mtime = get_mtime(&path);
            let poll_interval = Duration::from_secs(debounce_secs.max(1));
            let mut interval = tokio::time::interval(poll_interval);
            interval.tick().await; // Skip first

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => break,
                    _ = interval.tick() => {
                        let current_mtime = get_mtime(&path);

                        if current_mtime == last_mtime {
                            continue;
                        }
                        last_mtime = current_mtime;

                        let pipeline_id = pipeline_clone.id.clone();
                        let pipeline_name = pipeline_clone.name.clone();

                        {
                            let mut running = running_pipelines.lock().await;
                            if running.contains(&pipeline_id) {
                                continue;
                            }
                            running.insert(pipeline_id.clone());
                        }

                        let input = format!("File changed: {watch_path}");
                        let run = execute_pipeline_full(
                            &pipeline_clone,
                            Some(input),
                            &config,
                            tokio_util::sync::CancellationToken::new(),
                            Some(notify_tx.clone()),
                        )
                        .await;

                        let summary = if run.status == "success" {
                            let c = run.step_runs.iter().filter(|s| s.status == "success").count();
                            format!("{c} steps completed successfully")
                        } else {
                            run.step_runs.iter().find(|s| s.status == "error")
                                .and_then(|s| s.error.clone())
                                .unwrap_or_else(|| format!("Pipeline {}", run.status))
                        };

                        let _ = notify_tx.send(PipelineNotification {
                            pipeline_id: pipeline_id.clone(),
                            pipeline_name,
                            status: run.status,
                            summary,
                        });

                        running_pipelines.lock().await.remove(&pipeline_id);
                    }
                }
            }
        });

        let mut timers = self.timers.lock().await;
        if let Some(old) = timers.insert(pipeline.id.clone(), handle) {
            old.abort();
        }
        debug!(
            pipeline_id = %pipeline.id,
            path = %watch_path_for_log,
            debounce_secs,
            "Started file watcher for pipeline"
        );
    }
}
