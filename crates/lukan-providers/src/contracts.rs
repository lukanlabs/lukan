use async_trait::async_trait;
use lukan_core::models::events::StreamEvent;
use lukan_core::models::messages::Message;
use lukan_core::models::tools::ToolDefinition;
use tokio::sync::mpsc;

/// System prompt with caching support
#[derive(Debug, Clone)]
pub enum SystemPrompt {
    /// Simple text prompt
    Text(String),
    /// Structured prompt with cacheable and dynamic parts
    Structured {
        /// Blocks ordered by stability (most stable first) — each gets cache_control
        cached: Vec<String>,
        /// Dynamic content that changes every call
        dynamic: String,
    },
}

/// Parameters for a streaming LLM request
pub struct StreamParams {
    pub system_prompt: SystemPrompt,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
}

/// Common interface for all LLM providers
#[async_trait]
pub trait Provider: Send + Sync {
    /// Provider name identifier
    fn name(&self) -> &str;

    /// Whether this provider supports image inputs
    fn supports_images(&self) -> bool {
        false
    }

    /// Set the reasoning effort level (only relevant for reasoning models).
    /// Valid values: "low", "medium", "high", "extra_high".
    fn set_reasoning_effort(&self, _effort: &str) {}

    /// Get the current reasoning effort level, if the provider supports it.
    fn reasoning_effort(&self) -> Option<&'static str> {
        None
    }

    /// Stream a response from the LLM, sending events through the channel
    async fn stream(
        &self,
        params: StreamParams,
        tx: mpsc::Sender<StreamEvent>,
    ) -> anyhow::Result<()>;
}
