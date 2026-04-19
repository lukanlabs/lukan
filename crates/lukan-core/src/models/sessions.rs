use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::checkpoints::Checkpoint;
use super::messages::Message;

/// A persisted chat session
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSession {
    pub id: String,
    pub name: Option<String>,
    pub messages: Vec<Message>,
    pub checkpoints: Vec<Checkpoint>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub provider: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub total_input_tokens: u64,
    #[serde(default)]
    pub total_output_tokens: u64,
    #[serde(default)]
    pub compaction_count: u32,
    /// Tokens at last memory update (to track when next update is due)
    #[serde(default)]
    pub last_memory_update_tokens: u64,
    /// Last context size (input tokens from most recent LLM call)
    #[serde(default)]
    pub last_context_size: u64,
    /// Summary from last compaction
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_summary: Option<String>,
    /// Working directory where the session was created
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Stable project root used to group sessions across worktrees of the same repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

/// Summary of a session for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub name: Option<String>,
    pub message_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Last user message (truncated) for display in session picker
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_message: Option<String>,
    /// Working directory where the session was created
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Stable project root used to group sessions across worktrees of the same repo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<String>,
}

impl ChatSession {
    pub fn new(id: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            name: None,
            messages: Vec::new(),
            checkpoints: Vec::new(),
            created_at: now,
            updated_at: now,
            provider: None,
            model: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            compaction_count: 0,
            last_memory_update_tokens: 0,
            last_context_size: 0,
            compaction_summary: None,
            cwd: std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string()),
            project_root: None,
        }
    }

    pub fn summary(&self) -> SessionSummary {
        use super::messages::Role;
        // Find last user message for preview
        let last_message = self
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|m| {
                let text = m.content.to_text();
                let first_line = text.lines().next().unwrap_or("");
                if first_line.len() > 60 {
                    let end = first_line.floor_char_boundary(57);
                    format!("{}…", &first_line[..end])
                } else {
                    first_line.to_string()
                }
            })
            .filter(|s| !s.is_empty());

        SessionSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            message_count: self.messages.len(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            provider: self.provider.clone(),
            model: self.model.clone(),
            last_message,
            cwd: self.cwd.clone(),
            project_root: self.project_root.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::messages::{Message, MessageContent, Role};
    use super::*;

    #[test]
    fn test_chat_session_new() {
        let session = ChatSession::new("test-id".into());
        assert_eq!(session.id, "test-id");
        assert!(session.name.is_none());
        assert!(session.messages.is_empty());
        assert!(session.checkpoints.is_empty());
        assert_eq!(session.total_input_tokens, 0);
        assert_eq!(session.total_output_tokens, 0);
        assert_eq!(session.compaction_count, 0);
        assert!(session.provider.is_none());
        assert!(session.model.is_none());
    }

    #[test]
    fn test_chat_session_new_timestamps() {
        let before = chrono::Utc::now();
        let session = ChatSession::new("s1".into());
        let after = chrono::Utc::now();
        assert!(session.created_at >= before);
        assert!(session.created_at <= after);
        assert_eq!(session.created_at, session.updated_at);
    }

    #[test]
    fn test_chat_session_summary_empty() {
        let session = ChatSession::new("s1".into());
        let summary = session.summary();
        assert_eq!(summary.id, "s1");
        assert!(summary.name.is_none());
        assert_eq!(summary.message_count, 0);
        assert!(summary.last_message.is_none());
    }

    #[test]
    fn test_chat_session_summary_with_messages() {
        let mut session = ChatSession::new("s2".into());
        session.messages.push(Message::user("Hello world"));
        session.messages.push(Message::assistant("Hi there!"));
        session.messages.push(Message::user("How are you?"));

        let summary = session.summary();
        assert_eq!(summary.message_count, 3);
        // Last user message should be "How are you?"
        assert_eq!(summary.last_message.as_deref(), Some("How are you?"));
    }

    #[test]
    fn test_chat_session_summary_truncates_long_message() {
        let mut session = ChatSession::new("s3".into());
        let long_msg = "a".repeat(100);
        session.messages.push(Message::user(long_msg));

        let summary = session.summary();
        let last = summary.last_message.unwrap();
        // Should be truncated with ellipsis
        assert!(last.len() <= 61); // 57 chars + "…" (3 bytes)
        assert!(last.ends_with('…'));
    }

    #[test]
    fn test_chat_session_summary_skips_empty_user_messages() {
        let mut session = ChatSession::new("s4".into());
        session.messages.push(Message {
            role: Role::User,
            content: MessageContent::Text("".into()),
            tool_call_id: None,
            name: None,
        });

        let summary = session.summary();
        // Empty message should be filtered out
        assert!(summary.last_message.is_none());
    }

    #[test]
    fn test_chat_session_serde_roundtrip() {
        let mut session = ChatSession::new("s5".into());
        session.name = Some("Test Session".into());
        session.provider = Some("anthropic".into());
        session.model = Some("claude-3".into());
        session.total_input_tokens = 1000;
        session.total_output_tokens = 500;
        session.messages.push(Message::user("test"));

        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains(r#""id":"s5""#));
        assert!(json.contains(r#""name":"Test Session""#));

        let parsed: ChatSession = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "s5");
        assert_eq!(parsed.name.as_deref(), Some("Test Session"));
        assert_eq!(parsed.total_input_tokens, 1000);
        assert_eq!(parsed.messages.len(), 1);
    }

    #[test]
    fn test_chat_session_serde_defaults_on_missing_fields() {
        // Simulate an older session format missing newer fields
        let json = r#"{
            "id": "old",
            "name": null,
            "messages": [],
            "checkpoints": [],
            "createdAt": "2024-01-01T00:00:00Z",
            "updatedAt": "2024-01-01T00:00:00Z",
            "provider": null,
            "model": null
        }"#;
        let session: ChatSession = serde_json::from_str(json).unwrap();
        assert_eq!(session.total_input_tokens, 0);
        assert_eq!(session.total_output_tokens, 0);
        assert_eq!(session.compaction_count, 0);
        assert_eq!(session.last_memory_update_tokens, 0);
        assert_eq!(session.last_context_size, 0);
        assert!(session.compaction_summary.is_none());
    }

    #[test]
    fn test_session_summary_serde() {
        let summary = SessionSummary {
            id: "sum1".into(),
            name: Some("My Session".into()),
            message_count: 10,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            provider: Some("anthropic".into()),
            model: Some("claude-3".into()),
            last_message: Some("hello".into()),
            cwd: None,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains(r#""messageCount":10"#));

        let parsed: SessionSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "sum1");
        assert_eq!(parsed.message_count, 10);
    }
}
