use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A checkpoint recording file changes during a session
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub id: String,
    pub message: String,
    pub snapshots: Vec<FileSnapshot>,
    pub created_at: DateTime<Utc>,
}

/// Snapshot of a single file change
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileSnapshot {
    pub path: String,
    pub operation: FileOperation,
    pub before: Option<String>,
    pub after: Option<String>,
    pub diff: Option<String>,
    #[serde(default)]
    pub additions: u32,
    #[serde(default)]
    pub deletions: u32,
}

/// Type of file operation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileOperation {
    Created,
    Modified,
    Deleted,
}
