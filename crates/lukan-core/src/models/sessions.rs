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
        }
    }

    pub fn summary(&self) -> SessionSummary {
        SessionSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            message_count: self.messages.len(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            provider: self.provider.clone(),
            model: self.model.clone(),
        }
    }
}
