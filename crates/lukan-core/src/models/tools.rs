use serde::{Deserialize, Serialize};

use super::checkpoints::FileSnapshot;

/// Definition of a tool that the LLM can call
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    #[serde(default)]
    pub deferred: bool,
    #[serde(default)]
    pub read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_hint: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::super::checkpoints::FileOperation;
    use super::*;

    #[test]
    fn test_tool_definition_serde() {
        let def = ToolDefinition {
            name: "Bash".into(),
            description: "Execute a shell command".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string"}
                }
            }),
        };
        let json = serde_json::to_string(&def).unwrap();
        assert!(json.contains(r#""name":"Bash""#));
        assert!(json.contains(r#""inputSchema""#));

        let parsed: ToolDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "Bash");
        assert_eq!(parsed.description, "Execute a shell command");
    }

    #[test]
    fn test_tool_result_success() {
        let result = ToolResult::success("file created");
        assert_eq!(result.content, "file created");
        assert!(!result.is_error);
        assert!(result.diff.is_none());
        assert!(result.image.is_none());
        assert!(result.snapshot.is_none());
    }

    #[test]
    fn test_tool_result_error() {
        let result = ToolResult::error("command failed");
        assert_eq!(result.content, "command failed");
        assert!(result.is_error);
    }

    #[test]
    fn test_tool_result_with_diff() {
        let result = ToolResult::success("edited").with_diff("--- a\n+++ b\n".into());
        assert_eq!(result.diff.as_deref(), Some("--- a\n+++ b\n"));
        assert!(!result.is_error);
    }

    #[test]
    fn test_tool_result_with_snapshot() {
        let snapshot = FileSnapshot {
            path: "/tmp/test.txt".into(),
            operation: FileOperation::Created,
            before: None,
            after: Some("content".into()),
            diff: None,
            additions: 1,
            deletions: 0,
        };
        let result = ToolResult::success("created").with_snapshot(snapshot);
        assert!(result.snapshot.is_some());
        assert_eq!(result.snapshot.unwrap().path, "/tmp/test.txt");
    }

    #[test]
    fn test_tool_result_chaining() {
        let snapshot = FileSnapshot {
            path: "/tmp/f.txt".into(),
            operation: FileOperation::Modified,
            before: Some("old".into()),
            after: Some("new".into()),
            diff: Some("diff".into()),
            additions: 1,
            deletions: 1,
        };
        let result = ToolResult::success("done")
            .with_diff("diff-text".into())
            .with_snapshot(snapshot);
        assert_eq!(result.content, "done");
        assert!(!result.is_error);
        assert_eq!(result.diff.as_deref(), Some("diff-text"));
        assert!(result.snapshot.is_some());
    }
}
