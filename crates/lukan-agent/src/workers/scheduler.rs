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
use lukan_tools::create_default_registry;

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

/// Scheduler that manages worker timers and execution
pub struct WorkerScheduler {
    config: Mutex<ResolvedConfig>,
    timers: Mutex<HashMap<String, JoinHandle<()>>>,
    running_workers: Arc<Mutex<HashSet<String>>>,
    cancel_token: CancellationToken,
    notify_tx: broadcast::Sender<WorkerNotification>,
    started: AtomicBool,
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
        match WorkerManager::list().await {
            Ok(workers) => {
                for worker in workers {
                    if worker.enabled {
                        self.schedule_worker(&worker).await;
                    }
                }
                info!(
                    count = self.timers.lock().await.len(),
                    "Worker scheduler started"
                );
            }
            Err(e) => {
                error!(error = %e, "Failed to load workers on scheduler start");
            }
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
    let mut registry = create_default_registry();
    if let Some(tool_names) = tools_filter {
        let refs: Vec<&str> = tool_names.iter().map(|s| s.as_str()).collect();
        registry.retain(&refs);
    }

    let provider_name = config.config.provider.to_string();
    let model_name = config.effective_model();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

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
    };

    // Create agent and run
    match AgentLoop::new(agent_config).await {
        Ok(mut agent) => {
            let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);

            let prompt_owned = prompt.to_string();
            let agent_handle = tokio::spawn(async move {
                let result = agent.run_turn(&prompt_owned, event_tx).await;
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
