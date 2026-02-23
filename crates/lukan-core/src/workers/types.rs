use serde::{Deserialize, Serialize};

/// A worker definition: scheduled autonomous agent
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerDefinition {
    pub id: String,
    pub name: String,
    /// Schedule string: "every:5m" or "*/10 * * * *"
    pub schedule: String,
    pub prompt: String,
    /// Tool filter — None = all tools
    pub tools: Option<Vec<String>>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub enabled: bool,
    /// Notification channels: ["web", "whatsapp"]
    pub notify: Option<Vec<String>>,
    pub created_at: String,
    pub last_run_at: Option<String>,
    /// "success" | "error"
    pub last_run_status: Option<String>,
}

/// A single execution of a worker
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRun {
    pub id: String,
    pub worker_id: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    /// "running" | "success" | "error"
    pub status: String,
    pub output: String,
    pub error: Option<String>,
    pub token_usage: WorkerTokenUsage,
    pub turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WorkerTokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

/// Input for creating a new worker
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerCreateInput {
    pub name: String,
    pub schedule: String,
    pub prompt: String,
    pub tools: Option<Vec<String>>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub enabled: Option<bool>,
    pub notify: Option<Vec<String>>,
}

/// Input for updating an existing worker
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerUpdateInput {
    pub name: Option<String>,
    pub schedule: Option<String>,
    pub prompt: Option<String>,
    pub tools: Option<Vec<String>>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub enabled: Option<bool>,
    pub notify: Option<Vec<String>>,
}

/// Worker with recent run status for list views
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerSummary {
    #[serde(flatten)]
    pub definition: WorkerDefinition,
    pub recent_run_status: Option<String>,
}

/// Worker detail: definition + recent runs
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerDetail {
    #[serde(flatten)]
    pub summary: WorkerSummary,
    pub recent_runs: Vec<WorkerRun>,
}
