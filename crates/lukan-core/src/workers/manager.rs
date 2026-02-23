use anyhow::Result;
use rand::Rng;
use tokio::sync::Mutex;
use tracing::debug;

use crate::config::LukanPaths;

use super::types::{
    WorkerCreateInput, WorkerDefinition, WorkerDetail, WorkerRun, WorkerSummary, WorkerUpdateInput,
};

static WRITE_LOCK: Mutex<()> = Mutex::const_new(());

/// Manages worker persistence to disk.
///
/// Workers list: ~/.config/lukan/workers.json
/// Run history:  ~/.config/lukan/workers/{id}/{run_id}.json
pub struct WorkerManager;

impl WorkerManager {
    /// List all worker definitions
    pub async fn list() -> Result<Vec<WorkerDefinition>> {
        let path = LukanPaths::workers_file();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = tokio::fs::read_to_string(&path).await?;
        let workers: Vec<WorkerDefinition> = serde_json::from_str(&data)?;
        Ok(workers)
    }

    /// Get a single worker by ID
    pub async fn get(id: &str) -> Result<Option<WorkerDefinition>> {
        let workers = Self::list().await?;
        Ok(workers.into_iter().find(|w| w.id == id))
    }

    /// Create a new worker
    pub async fn create(input: WorkerCreateInput) -> Result<WorkerDefinition> {
        let _lock = WRITE_LOCK.lock().await;

        // Validate schedule
        super::schedule::parse_schedule_ms(&input.schedule)?;

        let worker = WorkerDefinition {
            id: generate_id(),
            name: input.name,
            schedule: input.schedule,
            prompt: input.prompt,
            tools: input.tools,
            provider: input.provider,
            model: input.model,
            enabled: input.enabled.unwrap_or(true),
            notify: input.notify,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_run_at: None,
            last_run_status: None,
        };

        let mut workers = Self::list().await.unwrap_or_default();
        workers.push(worker.clone());
        Self::save_list(&workers).await?;

        debug!(id = %worker.id, name = %worker.name, "Created worker");
        Ok(worker)
    }

    /// Update a worker. Returns None if not found.
    pub async fn update(id: &str, patch: WorkerUpdateInput) -> Result<Option<WorkerDefinition>> {
        let _lock = WRITE_LOCK.lock().await;

        let mut workers = Self::list().await?;
        let Some(w) = workers.iter_mut().find(|w| w.id == id) else {
            return Ok(None);
        };

        if let Some(name) = patch.name {
            w.name = name;
        }
        if let Some(schedule) = patch.schedule {
            super::schedule::parse_schedule_ms(&schedule)?;
            w.schedule = schedule;
        }
        if let Some(prompt) = patch.prompt {
            w.prompt = prompt;
        }
        if let Some(tools) = patch.tools {
            w.tools = Some(tools);
        }
        if let Some(provider) = patch.provider {
            w.provider = Some(provider);
        }
        if let Some(model) = patch.model {
            w.model = Some(model);
        }
        if let Some(enabled) = patch.enabled {
            w.enabled = enabled;
        }
        if let Some(notify) = patch.notify {
            w.notify = Some(notify);
        }

        let updated = w.clone();
        Self::save_list(&workers).await?;
        debug!(id, "Updated worker");
        Ok(Some(updated))
    }

    /// Delete a worker and its run history. Returns true if found.
    pub async fn delete(id: &str) -> Result<bool> {
        let _lock = WRITE_LOCK.lock().await;

        let mut workers = Self::list().await?;
        let len_before = workers.len();
        workers.retain(|w| w.id != id);

        if workers.len() == len_before {
            return Ok(false);
        }

        Self::save_list(&workers).await?;

        // Clean up run history directory
        let runs_dir = LukanPaths::worker_runs_dir(id);
        if runs_dir.exists() {
            tokio::fs::remove_dir_all(&runs_dir).await.ok();
        }

        debug!(id, "Deleted worker");
        Ok(true)
    }

    /// Save a worker run to disk
    pub async fn save_run(run: &WorkerRun) -> Result<()> {
        let dir = LukanPaths::worker_runs_dir(&run.worker_id);
        tokio::fs::create_dir_all(&dir).await?;
        let path = LukanPaths::worker_run_file(&run.worker_id, &run.id);
        let data = serde_json::to_string_pretty(run)?;
        tokio::fs::write(&path, data).await?;
        debug!(worker_id = %run.worker_id, run_id = %run.id, "Saved worker run");
        Ok(())
    }

    /// Get recent runs for a worker, sorted by started_at descending
    pub async fn get_runs(worker_id: &str, limit: usize) -> Result<Vec<WorkerRun>> {
        let dir = LukanPaths::worker_runs_dir(worker_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = tokio::fs::read_dir(&dir).await?;
        let mut runs = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(data) = tokio::fs::read_to_string(&path).await
                && let Ok(run) = serde_json::from_str::<WorkerRun>(&data)
            {
                runs.push(run);
            }
        }

        // Sort by started_at descending
        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        runs.truncate(limit);
        Ok(runs)
    }

    /// Get a specific run
    pub async fn get_run(worker_id: &str, run_id: &str) -> Result<Option<WorkerRun>> {
        let path = LukanPaths::worker_run_file(worker_id, run_id);
        if !path.exists() {
            return Ok(None);
        }
        let data = tokio::fs::read_to_string(&path).await?;
        let run: WorkerRun = serde_json::from_str(&data)?;
        Ok(Some(run))
    }

    /// Prune old runs, keeping only the most recent `keep` entries
    pub async fn prune_runs(worker_id: &str, keep: usize) -> Result<()> {
        let runs = Self::get_runs(worker_id, usize::MAX).await?;
        if runs.len() <= keep {
            return Ok(());
        }
        // Runs are already sorted desc — remove everything after `keep`
        for run in &runs[keep..] {
            let path = LukanPaths::worker_run_file(worker_id, &run.id);
            tokio::fs::remove_file(&path).await.ok();
        }
        debug!(worker_id, pruned = runs.len() - keep, "Pruned old worker runs");
        Ok(())
    }

    /// Get worker summaries for list views
    pub async fn get_summaries() -> Result<Vec<WorkerSummary>> {
        let workers = Self::list().await?;
        let mut summaries = Vec::with_capacity(workers.len());
        for w in workers {
            let recent_run_status = w.last_run_status.clone();
            summaries.push(WorkerSummary {
                definition: w,
                recent_run_status,
            });
        }
        Ok(summaries)
    }

    /// Get full worker detail with recent runs
    pub async fn get_detail(id: &str) -> Result<Option<WorkerDetail>> {
        let Some(worker) = Self::get(id).await? else {
            return Ok(None);
        };
        let recent_runs = Self::get_runs(id, 20).await?;
        let recent_run_status = worker.last_run_status.clone();
        Ok(Some(WorkerDetail {
            summary: WorkerSummary {
                definition: worker,
                recent_run_status,
            },
            recent_runs,
        }))
    }

    /// Update a worker's last_run_at and last_run_status fields
    pub async fn update_last_run(id: &str, status: &str) -> Result<()> {
        let _lock = WRITE_LOCK.lock().await;

        let mut workers = Self::list().await?;
        if let Some(w) = workers.iter_mut().find(|w| w.id == id) {
            w.last_run_at = Some(chrono::Utc::now().to_rfc3339());
            w.last_run_status = Some(status.to_string());
            Self::save_list(&workers).await?;
        }
        Ok(())
    }

    // ── Private helpers ──

    async fn save_list(workers: &[WorkerDefinition]) -> Result<()> {
        LukanPaths::ensure_dirs().await?;
        let path = LukanPaths::workers_file();
        let data = serde_json::to_string_pretty(workers)?;
        tokio::fs::write(&path, data).await?;
        Ok(())
    }
}

/// Generate a random 6-char hex string
fn generate_id() -> String {
    let bytes: [u8; 3] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
