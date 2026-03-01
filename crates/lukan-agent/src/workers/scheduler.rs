use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use lukan_core::config::ResolvedConfig;
use lukan_core::models::events::StreamEvent;
use lukan_core::workers::{
    WorkerCreateInput, WorkerDefinition, WorkerDetail, WorkerManager, WorkerRun, WorkerSummary,
    WorkerTokenUsage, WorkerUpdateInput,
};
use lukan_providers::{SystemPrompt, create_provider};
use lukan_tools::create_configured_registry;

use crate::{AgentConfig, AgentLoop};

const MAX_RUNS_KEPT: usize = 20;

/// Notification emitted when a worker run completes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerNotification {
    pub worker_id: String,
    pub worker_name: String,
    pub status: String,
    pub summary: String,
}

/// Key fields used to detect when a worker needs rescheduling
#[derive(Clone, PartialEq)]
struct WorkerSnapshot {
    enabled: bool,
    schedule: String,
    prompt: String,
    tools: Option<Vec<String>>,
    provider: Option<String>,
    model: Option<String>,
}

impl From<&WorkerDefinition> for WorkerSnapshot {
    fn from(w: &WorkerDefinition) -> Self {
        Self {
            enabled: w.enabled,
            schedule: w.schedule.clone(),
            prompt: w.prompt.clone(),
            tools: w.tools.clone(),
            provider: w.provider.clone(),
            model: w.model.clone(),
        }
    }
}

/// Scheduler that manages worker timers and execution
pub struct WorkerScheduler {
    config: Mutex<ResolvedConfig>,
    timers: Mutex<HashMap<String, JoinHandle<()>>>,
    running_workers: Arc<Mutex<HashSet<String>>>,
    cancel_token: CancellationToken,
    notify_tx: broadcast::Sender<WorkerNotification>,
    started: AtomicBool,
    /// Last-known worker state for diffing during reload
    known_state: Mutex<HashMap<String, WorkerSnapshot>>,
}

impl WorkerScheduler {
    pub fn new(config: ResolvedConfig) -> Self {
        let (notify_tx, _) = broadcast::channel(64);
        Self {
            config: Mutex::new(config),
            timers: Mutex::new(HashMap::new()),
            running_workers: Arc::new(Mutex::new(HashSet::new())),
            cancel_token: CancellationToken::new(),
            notify_tx,
            started: AtomicBool::new(false),
            known_state: Mutex::new(HashMap::new()),
        }
    }

    /// Subscribe to worker notifications (run completions)
    pub fn subscribe(&self) -> broadcast::Receiver<WorkerNotification> {
        self.notify_tx.subscribe()
    }

    /// Start the scheduler: load all enabled workers and schedule them
    pub async fn start(&self) {
        if self.started.swap(true, Ordering::SeqCst) {
            return; // Already started
        }

        info!("Starting worker scheduler");
        // Clean up any runs stuck in "running" from previous crashes
        if let Err(e) = WorkerManager::cleanup_stale_runs().await {
            error!(error = %e, "Failed to cleanup stale worker runs");
        }
        match WorkerManager::list().await {
            Ok(workers) => {
                let mut known = self.known_state.lock().await;
                for worker in &workers {
                    known.insert(worker.id.clone(), WorkerSnapshot::from(worker));
                    if worker.enabled {
                        self.schedule_worker(worker).await;
                    }
                }
                let count = self.timers.lock().await.len();
                info!(count, "Worker scheduler started");
            }
            Err(e) => {
                error!(error = %e, "Failed to load workers on scheduler start");
            }
        }
    }

    /// Reload workers from disk and reconcile timers.
    /// Called periodically by the daemon to pick up external changes.
    pub async fn reload(&self) {
        let workers = match WorkerManager::list().await {
            Ok(w) => w,
            Err(e) => {
                error!(error = %e, "Failed to reload workers");
                return;
            }
        };

        let new_map: HashMap<String, &WorkerDefinition> =
            workers.iter().map(|w| (w.id.clone(), w)).collect();

        let mut known = self.known_state.lock().await;
        let mut timers = self.timers.lock().await;

        // Cancel timers for removed or newly-disabled workers
        let timer_ids: Vec<String> = timers.keys().cloned().collect();
        for id in &timer_ids {
            let should_cancel = match new_map.get(id) {
                None => true,                  // removed
                Some(w) if !w.enabled => true, // disabled
                _ => false,
            };
            if should_cancel && let Some(handle) = timers.remove(id) {
                handle.abort();
                debug!(worker_id = %id, "Cancelled timer (reload)");
            }
        }

        // Collect workers that need (re)scheduling
        let mut to_schedule: Vec<WorkerDefinition> = Vec::new();
        for w in &workers {
            if !w.enabled {
                // Ensure known_state reflects disabled state
                known.insert(w.id.clone(), WorkerSnapshot::from(w));
                continue;
            }

            let snap = WorkerSnapshot::from(w);
            let needs_schedule = match known.get(&w.id) {
                None => true, // New worker
                Some(old) => *old != snap,
            };

            if needs_schedule {
                // Cancel old timer if present
                if let Some(handle) = timers.remove(&w.id) {
                    handle.abort();
                }
                to_schedule.push(w.clone());
            }
            known.insert(w.id.clone(), snap);
        }

        // Remove known entries for workers no longer in the file
        known.retain(|id, _| new_map.contains_key(id));

        // Drop locks before scheduling (schedule_worker acquires timers lock)
        drop(timers);
        drop(known);

        for w in &to_schedule {
            self.schedule_worker(w).await;
            debug!(worker_id = %w.id, "Rescheduled worker (reload)");
        }
    }

    /// Stop all timers and running workers
    pub fn stop(&self) {
        if !self.started.swap(false, Ordering::SeqCst) {
            return;
        }
        info!("Stopping worker scheduler");
        self.cancel_token.cancel();
    }

    // ── CRUD pass-throughs ──

    pub async fn list_workers(&self) -> Result<Vec<WorkerSummary>> {
        WorkerManager::get_summaries().await
    }

    pub async fn create_worker(&self, input: WorkerCreateInput) -> Result<WorkerDefinition> {
        let worker = WorkerManager::create(input).await?;
        if worker.enabled {
            self.schedule_worker(&worker).await;
        }
        Ok(worker)
    }

    pub async fn update_worker(
        &self,
        id: &str,
        patch: WorkerUpdateInput,
    ) -> Result<Option<WorkerDefinition>> {
        let result = WorkerManager::update(id, patch).await?;
        if let Some(ref w) = result {
            self.reschedule_worker(w).await;
        }
        Ok(result)
    }

    pub async fn delete_worker(&self, id: &str) -> Result<bool> {
        self.cancel_timer(id).await;
        WorkerManager::delete(id).await
    }

    pub async fn toggle_worker(&self, id: &str, enabled: bool) -> Result<Option<WorkerDefinition>> {
        let patch = WorkerUpdateInput {
            enabled: Some(enabled),
            name: None,
            schedule: None,
            prompt: None,
            tools: None,
            provider: None,
            model: None,
            notify: None,
        };
        let result = WorkerManager::update(id, patch).await?;
        if let Some(ref w) = result {
            self.reschedule_worker(w).await;
        }
        Ok(result)
    }

    pub async fn get_worker_detail(&self, id: &str) -> Result<Option<WorkerDetail>> {
        WorkerManager::get_detail(id).await
    }

    pub async fn get_worker_run_detail(
        &self,
        worker_id: &str,
        run_id: &str,
    ) -> Result<Option<WorkerRun>> {
        WorkerManager::get_run(worker_id, run_id).await
    }

    // ── Internal scheduling ──

    async fn schedule_worker(&self, worker: &WorkerDefinition) {
        let interval_ms = match lukan_core::workers::schedule::parse_schedule_ms(&worker.schedule) {
            Ok(ms) => ms,
            Err(e) => {
                error!(
                    worker_id = %worker.id,
                    schedule = %worker.schedule,
                    error = %e,
                    "Invalid schedule, not scheduling worker"
                );
                return;
            }
        };

        let worker_id = worker.id.clone();
        let worker_name = worker.name.clone();
        let worker_prompt = worker.prompt.clone();
        let worker_tools = worker.tools.clone();
        let worker_provider = worker.provider.clone();
        let worker_model = worker.model.clone();

        let config = self.config.lock().await.clone();
        let running_workers = Arc::clone(&self.running_workers);
        let notify_tx = self.notify_tx.clone();
        let cancel_token = self.cancel_token.clone();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
            // Skip the first immediate tick — don't run on start
            interval.tick().await;

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        debug!(worker_id = %worker_id, "Worker timer cancelled");
                        break;
                    }
                    _ = interval.tick() => {
                        run_worker(
                            &worker_id,
                            &worker_name,
                            &worker_prompt,
                            worker_tools.as_deref(),
                            worker_provider.as_deref(),
                            worker_model.as_deref(),
                            &config,
                            &running_workers,
                            &notify_tx,
                        ).await;
                    }
                }
            }
        });

        let mut timers = self.timers.lock().await;
        // Cancel previous timer if exists
        if let Some(old) = timers.insert(worker.id.clone(), handle) {
            old.abort();
        }
        debug!(
            worker_id = %worker.id,
            interval_ms,
            "Scheduled worker"
        );
    }

    async fn reschedule_worker(&self, worker: &WorkerDefinition) {
        self.cancel_timer(&worker.id).await;
        if worker.enabled {
            self.schedule_worker(worker).await;
        }
    }

    async fn cancel_timer(&self, id: &str) {
        let mut timers = self.timers.lock().await;
        if let Some(handle) = timers.remove(id) {
            handle.abort();
            debug!(worker_id = %id, "Cancelled worker timer");
        }
    }
}

/// Execute a single worker run
#[allow(clippy::too_many_arguments)]
async fn run_worker(
    worker_id: &str,
    worker_name: &str,
    prompt: &str,
    tools_filter: Option<&[String]>,
    provider_override: Option<&str>,
    model_override: Option<&str>,
    base_config: &ResolvedConfig,
    running_workers: &Mutex<HashSet<String>>,
    notify_tx: &broadcast::Sender<WorkerNotification>,
) {
    // Guard: skip if already running
    {
        let mut running = running_workers.lock().await;
        if running.contains(worker_id) {
            debug!(worker_id, "Worker already running, skipping tick");
            return;
        }
        running.insert(worker_id.to_string());
    }

    let run_id = generate_run_id();
    info!(worker_id, run_id = %run_id, worker_name, "Starting worker run");

    let mut run = WorkerRun {
        id: run_id.clone(),
        worker_id: worker_id.to_string(),
        started_at: chrono::Utc::now().to_rfc3339(),
        completed_at: None,
        status: "running".to_string(),
        output: String::new(),
        error: None,
        token_usage: WorkerTokenUsage::default(),
        turns: 0,
    };

    // Save initial "running" state
    if let Err(e) = WorkerManager::save_run(&run).await {
        error!(worker_id, error = %e, "Failed to save initial worker run");
    }

    // Build config with overrides
    let mut config = base_config.clone();
    if let Some(p) = provider_override
        && let Ok(pn) = serde_json::from_value(serde_json::Value::String(p.to_string()))
    {
        config.config.provider = pn;
    }
    if let Some(m) = model_override {
        config.config.model = Some(m.to_string());
    }

    // Create provider
    let provider = match create_provider(&config) {
        Ok(p) => p,
        Err(e) => {
            run.status = "error".to_string();
            run.error = Some(format!("Failed to create provider: {e}"));
            run.completed_at = Some(chrono::Utc::now().to_rfc3339());
            WorkerManager::save_run(&run).await.ok();
            WorkerManager::update_last_run(worker_id, "error")
                .await
                .ok();
            running_workers.lock().await.remove(worker_id);
            return;
        }
    };

    // Build tool registry with optional filter
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
    if let Some(tool_names) = tools_filter {
        let refs: Vec<&str> = tool_names.iter().map(|s| s.as_str()).collect();
        registry.retain(&refs);
    }

    let provider_name = config.config.provider.to_string();
    let model_name = config.effective_model().unwrap_or_default();

    let system_prompt = SystemPrompt::Text(
        "You are a scheduled worker agent. Execute the task described in the user message. \
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
        // Workers run unattended — skip all permission checks
        permission_mode: lukan_core::config::types::PermissionMode::Skip,
        permissions,
        approval_rx: None,
        plan_review_rx: None,
        planner_answer_rx: None,
        browser_tools: false,
        skip_session_save: true,
        vision_provider: None,
    };

    // Create agent and run
    match AgentLoop::new(agent_config).await {
        Ok(mut agent) => {
            let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);

            let prompt_owned = prompt.to_string();
            let agent_handle = tokio::spawn(async move {
                let result = agent.run_turn(&prompt_owned, event_tx, None, None).await;
                (agent, result)
            });

            // Consume events, accumulate output and token usage
            while let Some(event) = event_rx.recv().await {
                match &event {
                    StreamEvent::TextDelta { text } => {
                        run.output.push_str(text);
                    }
                    StreamEvent::Usage {
                        input_tokens,
                        output_tokens,
                        cache_creation_tokens,
                        cache_read_tokens,
                    } => {
                        run.token_usage.input += input_tokens;
                        run.token_usage.output += output_tokens;
                        if let Some(cc) = cache_creation_tokens {
                            run.token_usage.cache_creation += cc;
                        }
                        if let Some(cr) = cache_read_tokens {
                            run.token_usage.cache_read += cr;
                        }
                    }
                    StreamEvent::MessageEnd { .. } => {
                        run.turns += 1;
                    }
                    _ => {}
                }
            }

            match agent_handle.await {
                Ok((_agent, result)) => {
                    if let Err(e) = result {
                        run.status = "error".to_string();
                        run.error = Some(format!("{e}"));
                    } else {
                        run.status = "success".to_string();
                    }
                }
                Err(e) => {
                    run.status = "error".to_string();
                    run.error = Some(format!("Agent task panicked: {e}"));
                }
            }
        }
        Err(e) => {
            run.status = "error".to_string();
            run.error = Some(format!("Failed to create agent: {e}"));
        }
    }

    run.completed_at = Some(chrono::Utc::now().to_rfc3339());

    // Persist results
    WorkerManager::save_run(&run).await.ok();
    WorkerManager::update_last_run(worker_id, &run.status)
        .await
        .ok();
    WorkerManager::prune_runs(worker_id, MAX_RUNS_KEPT)
        .await
        .ok();

    // Emit notification
    let summary = if run.status == "success" {
        let preview: String = run.output.chars().take(200).collect();
        if preview.is_empty() {
            "Completed successfully".to_string()
        } else {
            preview
        }
    } else {
        run.error
            .clone()
            .unwrap_or_else(|| "Unknown error".to_string())
    };

    let notification = WorkerNotification {
        worker_id: worker_id.to_string(),
        worker_name: worker_name.to_string(),
        status: run.status.clone(),
        summary,
    };
    let _ = notify_tx.send(notification);

    info!(
        worker_id,
        run_id = %run.id,
        status = %run.status,
        turns = run.turns,
        "Worker run completed"
    );

    // Remove from running set
    running_workers.lock().await.remove(worker_id);
}

fn generate_run_id() -> String {
    let bytes: [u8; 3] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
