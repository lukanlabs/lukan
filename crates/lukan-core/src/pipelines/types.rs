use serde::{Deserialize, Serialize};

/// Trigger type for a pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PipelineTrigger {
    /// Triggered manually via UI or API
    Manual,
    /// Triggered on a schedule (same format as workers: "every:5m", "*/10 * * * *")
    Schedule { schedule: String },
}

/// Condition for a step connection
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StepCondition {
    /// Always follow this connection
    Always,
    /// Follow if the previous step's output contains this string
    Contains { value: String },
    /// Follow if the previous step's output matches this regex
    Matches { pattern: String },
    /// Follow based on the previous step's status
    Status { status: String },
}

/// A pipeline definition: DAG of agent steps
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineDefinition {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub trigger: PipelineTrigger,
    pub steps: Vec<PipelineStep>,
    pub connections: Vec<StepConnection>,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub last_run_at: Option<String>,
    /// "success" | "error" | "partial"
    pub last_run_status: Option<String>,
}

/// A single step in a pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineStep {
    pub id: String,
    pub name: String,
    pub prompt: String,
    /// Template with {{input}} or {{prev.step_id.output}} placeholders
    pub prompt_template: Option<String>,
    /// Tool filter — None = all tools
    pub tools: Option<Vec<String>>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub timeout_secs: Option<u64>,
    pub max_turns: Option<u32>,
    /// "stop" | "skip" | "retry:N"
    pub on_error: Option<String>,
}

/// A connection between two steps in the pipeline DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepConnection {
    /// Source step ID, or "__trigger__" for the pipeline input
    pub from_step: String,
    /// Destination step ID
    pub to_step: String,
    /// Condition to evaluate — None means always
    pub condition: Option<StepCondition>,
}

/// A single execution of a pipeline
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRun {
    pub id: String,
    pub pipeline_id: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    /// "running" | "success" | "error" | "partial"
    pub status: String,
    pub step_runs: Vec<StepRun>,
    pub trigger_input: Option<String>,
    pub token_usage: PipelineTokenUsage,
}

/// A single step execution within a pipeline run
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRun {
    pub step_id: String,
    pub step_name: String,
    /// "pending" | "running" | "success" | "error" | "skipped"
    pub status: String,
    /// Input fed to this step (output from upstream steps)
    pub input: Option<String>,
    pub output: String,
    pub error: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub token_usage: PipelineTokenUsage,
    pub turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PipelineTokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
}

/// Input for creating a new pipeline
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineCreateInput {
    pub name: String,
    pub description: Option<String>,
    pub trigger: PipelineTrigger,
    pub steps: Vec<PipelineStep>,
    pub connections: Vec<StepConnection>,
    pub enabled: Option<bool>,
}

/// Input for updating an existing pipeline
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineUpdateInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub trigger: Option<PipelineTrigger>,
    pub steps: Option<Vec<PipelineStep>>,
    pub connections: Option<Vec<StepConnection>>,
    pub enabled: Option<bool>,
}

/// Pipeline with recent run status for list views
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineSummary {
    #[serde(flatten)]
    pub definition: PipelineDefinition,
    pub recent_run_status: Option<String>,
}

/// Pipeline detail: definition + recent runs
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineDetail {
    #[serde(flatten)]
    pub summary: PipelineSummary,
    pub recent_runs: Vec<PipelineRun>,
}
