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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_worker_definition_serde_roundtrip() {
        let worker = WorkerDefinition {
            id: "abc123".into(),
            name: "Check logs".into(),
            schedule: "every:5m".into(),
            prompt: "Check for errors in logs".into(),
            tools: Some(vec!["Bash".into(), "ReadFiles".into()]),
            provider: Some("anthropic".into()),
            model: Some("claude-3".into()),
            enabled: true,
            notify: Some(vec!["web".into()]),
            created_at: "2024-06-01T00:00:00Z".into(),
            last_run_at: None,
            last_run_status: None,
        };
        let json = serde_json::to_string(&worker).unwrap();
        assert!(json.contains(r#""id":"abc123""#));
        assert!(json.contains(r#""schedule":"every:5m""#));

        let parsed: WorkerDefinition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "abc123");
        assert_eq!(parsed.name, "Check logs");
        assert!(parsed.enabled);
        assert_eq!(
            parsed.tools.as_deref(),
            Some(&["Bash".into(), "ReadFiles".into()][..])
        );
    }

    #[test]
    fn test_worker_run_serde_roundtrip() {
        let run = WorkerRun {
            id: "run-1".into(),
            worker_id: "w-1".into(),
            started_at: "2024-06-01T10:00:00Z".into(),
            completed_at: Some("2024-06-01T10:00:05Z".into()),
            status: "success".into(),
            output: "All clear".into(),
            error: None,
            token_usage: WorkerTokenUsage {
                input: 100,
                output: 50,
                cache_creation: 0,
                cache_read: 20,
            },
            turns: 3,
        };
        let json = serde_json::to_string(&run).unwrap();
        assert!(json.contains(r#""workerId":"w-1""#));
        assert!(json.contains(r#""turns":3"#));

        let parsed: WorkerRun = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.status, "success");
        assert_eq!(parsed.token_usage.input, 100);
        assert_eq!(parsed.token_usage.cache_read, 20);
    }

    #[test]
    fn test_worker_token_usage_default() {
        let usage = WorkerTokenUsage::default();
        assert_eq!(usage.input, 0);
        assert_eq!(usage.output, 0);
        assert_eq!(usage.cache_creation, 0);
        assert_eq!(usage.cache_read, 0);
    }

    #[test]
    fn test_worker_create_input_deserialize() {
        let json = r#"{
            "name": "Daily Report",
            "schedule": "every:1h",
            "prompt": "Generate report"
        }"#;
        let input: WorkerCreateInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.name, "Daily Report");
        assert_eq!(input.schedule, "every:1h");
        assert!(input.tools.is_none());
        assert!(input.enabled.is_none());
    }

    #[test]
    fn test_worker_update_input_partial() {
        let json = r#"{"name": "Renamed"}"#;
        let input: WorkerUpdateInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.name.as_deref(), Some("Renamed"));
        assert!(input.schedule.is_none());
        assert!(input.prompt.is_none());
        assert!(input.enabled.is_none());
    }

    #[test]
    fn test_worker_summary_flatten() {
        let summary = WorkerSummary {
            definition: WorkerDefinition {
                id: "w1".into(),
                name: "Test".into(),
                schedule: "every:5m".into(),
                prompt: "do stuff".into(),
                tools: None,
                provider: None,
                model: None,
                enabled: true,
                notify: None,
                created_at: "2024-01-01T00:00:00Z".into(),
                last_run_at: None,
                last_run_status: None,
            },
            recent_run_status: Some("success".into()),
        };
        let json = serde_json::to_string(&summary).unwrap();
        // Flatten should merge definition fields into the top level
        assert!(json.contains(r#""id":"w1""#));
        assert!(json.contains(r#""recentRunStatus":"success""#));
        // Should not have a nested "definition" key
        assert!(!json.contains(r#""definition""#));
    }

    #[test]
    fn test_worker_detail_structure() {
        let detail = WorkerDetail {
            summary: WorkerSummary {
                definition: WorkerDefinition {
                    id: "w1".into(),
                    name: "Test".into(),
                    schedule: "every:5m".into(),
                    prompt: "do stuff".into(),
                    tools: None,
                    provider: None,
                    model: None,
                    enabled: true,
                    notify: None,
                    created_at: "2024-01-01T00:00:00Z".into(),
                    last_run_at: None,
                    last_run_status: None,
                },
                recent_run_status: None,
            },
            recent_runs: vec![],
        };
        let json = serde_json::to_string(&detail).unwrap();
        assert!(json.contains(r#""recentRuns":[]"#));
        assert!(json.contains(r#""id":"w1""#));
    }
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
