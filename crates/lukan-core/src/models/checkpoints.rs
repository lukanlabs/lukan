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
    /// Index into the message history *before* this turn's user message was added.
    /// Used by the rewind system to truncate history back to this point.
    #[serde(default)]
    pub message_index: usize,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_operation_serde() {
        assert_eq!(
            serde_json::to_string(&FileOperation::Created).unwrap(),
            r#""created""#
        );
        assert_eq!(
            serde_json::to_string(&FileOperation::Modified).unwrap(),
            r#""modified""#
        );
        assert_eq!(
            serde_json::to_string(&FileOperation::Deleted).unwrap(),
            r#""deleted""#
        );

        let parsed: FileOperation = serde_json::from_str(r#""modified""#).unwrap();
        assert_eq!(parsed, FileOperation::Modified);
    }

    #[test]
    fn test_file_snapshot_serde_roundtrip() {
        let snapshot = FileSnapshot {
            path: "/home/user/file.rs".into(),
            operation: FileOperation::Modified,
            before: Some("old content".into()),
            after: Some("new content".into()),
            diff: Some("@@ -1 +1 @@\n-old\n+new".into()),
            additions: 1,
            deletions: 1,
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains(r#""operation":"modified""#));

        let parsed: FileSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.path, "/home/user/file.rs");
        assert_eq!(parsed.operation, FileOperation::Modified);
        assert_eq!(parsed.before.as_deref(), Some("old content"));
        assert_eq!(parsed.after.as_deref(), Some("new content"));
        assert_eq!(parsed.additions, 1);
        assert_eq!(parsed.deletions, 1);
    }

    #[test]
    fn test_file_snapshot_created_no_before() {
        let snapshot = FileSnapshot {
            path: "new_file.txt".into(),
            operation: FileOperation::Created,
            before: None,
            after: Some("content".into()),
            diff: None,
            additions: 5,
            deletions: 0,
        };
        let json = serde_json::to_string(&snapshot).unwrap();
        let parsed: FileSnapshot = serde_json::from_str(&json).unwrap();
        assert!(parsed.before.is_none());
        assert_eq!(parsed.after.as_deref(), Some("content"));
    }

    #[test]
    fn test_file_snapshot_defaults_on_missing() {
        let json =
            r#"{"path":"f.txt","operation":"deleted","before":"x","after":null,"diff":null}"#;
        let parsed: FileSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.additions, 0);
        assert_eq!(parsed.deletions, 0);
    }

    #[test]
    fn test_checkpoint_serde_roundtrip() {
        let checkpoint = Checkpoint {
            id: "cp-1".into(),
            message: "Added error handling".into(),
            snapshots: vec![FileSnapshot {
                path: "src/main.rs".into(),
                operation: FileOperation::Modified,
                before: Some("fn main() {}".into()),
                after: Some("fn main() -> Result<()> {}".into()),
                diff: Some("diff".into()),
                additions: 1,
                deletions: 1,
            }],
            created_at: chrono::Utc::now(),
            message_index: 5,
        };
        let json = serde_json::to_string(&checkpoint).unwrap();
        assert!(json.contains(r#""id":"cp-1""#));
        assert!(json.contains(r#""messageIndex":5"#));

        let parsed: Checkpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "cp-1");
        assert_eq!(parsed.message, "Added error handling");
        assert_eq!(parsed.snapshots.len(), 1);
        assert_eq!(parsed.message_index, 5);
    }

    #[test]
    fn test_checkpoint_message_index_default() {
        let json = r#"{
            "id": "cp-old",
            "message": "old checkpoint",
            "snapshots": [],
            "createdAt": "2024-01-01T00:00:00Z"
        }"#;
        let parsed: Checkpoint = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.message_index, 0);
    }
}
