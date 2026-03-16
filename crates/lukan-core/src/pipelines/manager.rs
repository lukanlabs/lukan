use anyhow::Result;
use rand::Rng;
use tokio::sync::Mutex;
use tracing::debug;

use crate::config::LukanPaths;

use super::types::{
    PipelineCreateInput, PipelineDefinition, PipelineDetail, PipelineRun, PipelineSummary,
    PipelineUpdateInput,
};

static WRITE_LOCK: Mutex<()> = Mutex::const_new(());

/// Manages pipeline persistence to disk.
///
/// Pipeline list: ~/.config/lukan/pipelines.json
/// Run history:   ~/.config/lukan/pipelines/{id}/{run_id}.json
pub struct PipelineManager;

impl PipelineManager {
    /// List all pipeline definitions
    pub async fn list() -> Result<Vec<PipelineDefinition>> {
        let path = LukanPaths::pipelines_file();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = tokio::fs::read_to_string(&path).await?;
        let pipelines: Vec<PipelineDefinition> = serde_json::from_str(&data)?;
        Ok(pipelines)
    }

    /// Get a single pipeline by ID
    pub async fn get(id: &str) -> Result<Option<PipelineDefinition>> {
        let pipelines = Self::list().await?;
        Ok(pipelines.into_iter().find(|p| p.id == id))
    }

    /// Create a new pipeline
    pub async fn create(input: PipelineCreateInput) -> Result<PipelineDefinition> {
        let _lock = WRITE_LOCK.lock().await;

        // Validate schedule if trigger is Schedule
        if let super::types::PipelineTrigger::Schedule { ref schedule } = input.trigger {
            crate::workers::schedule::parse_schedule_ms(schedule)?;
        }

        let pipeline = PipelineDefinition {
            id: generate_id(),
            name: input.name,
            description: input.description,
            enabled: input.enabled.unwrap_or(true),
            trigger: input.trigger,
            steps: input.steps,
            connections: input.connections,
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: None,
            last_run_at: None,
            last_run_status: None,
        };

        let mut pipelines = Self::list().await.unwrap_or_default();
        pipelines.push(pipeline.clone());
        Self::save_list(&pipelines).await?;

        debug!(id = %pipeline.id, name = %pipeline.name, "Created pipeline");
        Ok(pipeline)
    }

    /// Update a pipeline. Returns None if not found.
    pub async fn update(
        id: &str,
        patch: PipelineUpdateInput,
    ) -> Result<Option<PipelineDefinition>> {
        let _lock = WRITE_LOCK.lock().await;

        let mut pipelines = Self::list().await?;
        let Some(p) = pipelines.iter_mut().find(|p| p.id == id) else {
            return Ok(None);
        };

        if let Some(name) = patch.name {
            p.name = name;
        }
        if let Some(description) = patch.description {
            p.description = Some(description);
        }
        if let Some(trigger) = patch.trigger {
            if let super::types::PipelineTrigger::Schedule { ref schedule } = trigger {
                crate::workers::schedule::parse_schedule_ms(schedule)?;
            }
            p.trigger = trigger;
        }
        if let Some(steps) = patch.steps {
            p.steps = steps;
        }
        if let Some(connections) = patch.connections {
            p.connections = connections;
        }
        if let Some(enabled) = patch.enabled {
            p.enabled = enabled;
        }
        p.updated_at = Some(chrono::Utc::now().to_rfc3339());

        let updated = p.clone();
        Self::save_list(&pipelines).await?;
        debug!(id, "Updated pipeline");
        Ok(Some(updated))
    }

    /// Delete a pipeline and its run history. Returns true if found.
    pub async fn delete(id: &str) -> Result<bool> {
        let _lock = WRITE_LOCK.lock().await;

        let mut pipelines = Self::list().await?;
        let len_before = pipelines.len();
        pipelines.retain(|p| p.id != id);

        if pipelines.len() == len_before {
            return Ok(false);
        }

        Self::save_list(&pipelines).await?;

        // Clean up run history directory
        let runs_dir = LukanPaths::pipeline_runs_dir(id);
        if runs_dir.exists() {
            tokio::fs::remove_dir_all(&runs_dir).await.ok();
        }

        debug!(id, "Deleted pipeline");
        Ok(true)
    }

    /// Save a pipeline run to disk
    pub async fn save_run(run: &PipelineRun) -> Result<()> {
        let dir = LukanPaths::pipeline_runs_dir(&run.pipeline_id);
        tokio::fs::create_dir_all(&dir).await?;
        let path = LukanPaths::pipeline_run_file(&run.pipeline_id, &run.id);
        let data = serde_json::to_string_pretty(run)?;
        tokio::fs::write(&path, data).await?;
        debug!(pipeline_id = %run.pipeline_id, run_id = %run.id, "Saved pipeline run");
        Ok(())
    }

    /// Get recent runs for a pipeline, sorted by started_at descending
    pub async fn get_runs(pipeline_id: &str, limit: usize) -> Result<Vec<PipelineRun>> {
        let dir = LukanPaths::pipeline_runs_dir(pipeline_id);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = tokio::fs::read_dir(&dir).await?;
        let mut runs = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Ok(data) = tokio::fs::read_to_string(&path).await
                && let Ok(run) = serde_json::from_str::<PipelineRun>(&data)
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
    pub async fn get_run(pipeline_id: &str, run_id: &str) -> Result<Option<PipelineRun>> {
        let path = LukanPaths::pipeline_run_file(pipeline_id, run_id);
        if !path.exists() {
            return Ok(None);
        }
        let data = tokio::fs::read_to_string(&path).await?;
        let run: PipelineRun = serde_json::from_str(&data)?;
        Ok(Some(run))
    }

    /// Prune old runs, keeping only the most recent `keep` entries
    pub async fn prune_runs(pipeline_id: &str, keep: usize) -> Result<()> {
        let runs = Self::get_runs(pipeline_id, usize::MAX).await?;
        if runs.len() <= keep {
            return Ok(());
        }
        for run in &runs[keep..] {
            let path = LukanPaths::pipeline_run_file(pipeline_id, &run.id);
            tokio::fs::remove_file(&path).await.ok();
        }
        debug!(
            pipeline_id,
            pruned = runs.len() - keep,
            "Pruned old pipeline runs"
        );
        Ok(())
    }

    /// Mark any runs stuck in "running" status as "error" (interrupted).
    /// Runs with "waiting_approval" steps are preserved if the approval hasn't timed out.
    pub async fn cleanup_stale_runs() -> Result<()> {
        let pipelines = Self::list().await?;
        let now = chrono::Utc::now();

        for p in &pipelines {
            let runs = Self::get_runs(&p.id, usize::MAX).await?;
            for mut run in runs {
                if run.status == "running" {
                    // Check if any step is waiting_approval with a valid (non-expired) approval
                    let has_live_approval = run.step_runs.iter().any(|sr| {
                        if sr.status != "waiting_approval" {
                            return false;
                        }
                        if let Some(ref aid) = sr.approval_id
                            && let Ok(data) = std::fs::read_to_string(
                                crate::config::LukanPaths::approval_file(aid),
                            )
                            && let Ok(req) =
                                serde_json::from_str::<crate::approvals::ApprovalRequest>(&data)
                            && req.status == "pending"
                            && let Ok(timeout) =
                                chrono::DateTime::parse_from_rfc3339(&req.timeout_at)
                        {
                            return timeout > now;
                        }
                        false
                    });

                    if has_live_approval {
                        // Don't mark the run as error — it's still waiting for approval
                        // Just mark any interrupted "running" steps as error
                        for step_run in &mut run.step_runs {
                            if step_run.status == "running" {
                                step_run.status = "error".to_string();
                                step_run.error =
                                    Some("Interrupted (process restarted)".to_string());
                                step_run.completed_at = Some(chrono::Utc::now().to_rfc3339());
                            }
                        }
                        Self::save_run(&run).await.ok();
                        debug!(
                            pipeline_id = %p.id,
                            run_id = %run.id,
                            "Preserved pipeline run with active approval gate"
                        );
                    } else {
                        // No live approvals — mark everything as error
                        run.status = "error".to_string();
                        run.completed_at = Some(chrono::Utc::now().to_rfc3339());
                        for step_run in &mut run.step_runs {
                            if step_run.status == "running"
                                || step_run.status == "pending"
                                || step_run.status == "waiting_approval"
                            {
                                step_run.status = "error".to_string();
                                step_run.error =
                                    Some("Interrupted (process restarted)".to_string());
                                step_run.completed_at = Some(chrono::Utc::now().to_rfc3339());
                            }
                        }
                        Self::save_run(&run).await.ok();
                        debug!(
                            pipeline_id = %p.id,
                            run_id = %run.id,
                            "Marked stale running pipeline run as error"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Get pipeline summaries for list views
    pub async fn get_summaries() -> Result<Vec<PipelineSummary>> {
        let pipelines = Self::list().await?;
        let mut summaries = Vec::with_capacity(pipelines.len());
        for p in pipelines {
            let recent_run_status = p.last_run_status.clone();
            summaries.push(PipelineSummary {
                definition: p,
                recent_run_status,
            });
        }
        Ok(summaries)
    }

    /// Get full pipeline detail with recent runs
    pub async fn get_detail(id: &str) -> Result<Option<PipelineDetail>> {
        let Some(pipeline) = Self::get(id).await? else {
            return Ok(None);
        };
        let recent_runs = Self::get_runs(id, 20).await?;
        let recent_run_status = pipeline.last_run_status.clone();
        Ok(Some(PipelineDetail {
            summary: PipelineSummary {
                definition: pipeline,
                recent_run_status,
            },
            recent_runs,
        }))
    }

    /// Update a pipeline's last_run_at and last_run_status fields
    pub async fn update_last_run(id: &str, status: &str) -> Result<()> {
        let _lock = WRITE_LOCK.lock().await;

        let mut pipelines = Self::list().await?;
        if let Some(p) = pipelines.iter_mut().find(|p| p.id == id) {
            p.last_run_at = Some(chrono::Utc::now().to_rfc3339());
            p.last_run_status = Some(status.to_string());
            Self::save_list(&pipelines).await?;
        }
        Ok(())
    }

    // ── Private helpers ──

    async fn save_list(pipelines: &[PipelineDefinition]) -> Result<()> {
        LukanPaths::ensure_dirs().await?;
        let path = LukanPaths::pipelines_file();
        let data = serde_json::to_string_pretty(pipelines)?;
        tokio::fs::write(&path, data).await?;
        Ok(())
    }
}

/// Generate a random 6-char hex string
fn generate_id() -> String {
    let bytes: [u8; 3] = rand::rng().random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
