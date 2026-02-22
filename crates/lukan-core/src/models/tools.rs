use serde::{Deserialize, Serialize};

use super::checkpoints::FileSnapshot;

/// Definition of a tool that the LLM can call
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Result from executing a tool
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    pub diff: Option<String>,
    pub image: Option<String>,
    /// File snapshot for checkpoint tracking (WriteFile / EditFile)
    pub snapshot: Option<FileSnapshot>,
}

impl ToolResult {
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            diff: None,
            image: None,
            snapshot: None,
        }
    }

    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            diff: None,
            image: None,
            snapshot: None,
        }
    }

    pub fn with_diff(mut self, diff: String) -> Self {
        self.diff = Some(diff);
        self
    }

    pub fn with_snapshot(mut self, snapshot: FileSnapshot) -> Self {
        self.snapshot = Some(snapshot);
        self
    }
}
