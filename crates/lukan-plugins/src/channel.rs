use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::Result;
use lukan_agent::AgentLoop;
use lukan_core::config::LukanPaths;
use lukan_core::models::events::StreamEvent;
use lukan_core::models::plugin::{HostMessage, LogLevel, PluginMessage, PluginStatus};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Default max response length (characters) sent back to a plugin.
const DEFAULT_MAX_RESPONSE_LEN: usize = 4000;

/// Bridge between a plugin process and the AgentLoop.
///
/// Receives ChannelMessage from the plugin, runs the agent, and sends
/// AgentResponse back. Generalizes the WhatsApp channel pattern.
pub struct PluginChannel {
    name: String,
    max_response_len: usize,
    log_path: Option<PathBuf>,
    /// Last known mtime of the plugin's config.json (for hot-reload)
    config_mtime: Option<SystemTime>,
    /// Default tool names used when config.json has no "tools" key
    default_tools: Vec<String>,
}

impl PluginChannel {
    pub fn new(name: &str, max_response_len: Option<usize>, default_tools: Vec<String>) -> Self {
        let config_mtime = std::fs::metadata(LukanPaths::plugin_config(name))
            .and_then(|m| m.modified())
            .ok();
        Self {
            name: name.to_string(),
            max_response_len: max_response_len.unwrap_or(DEFAULT_MAX_RESPONSE_LEN),
            log_path: None,
            config_mtime,
            default_tools,
        }
    }

    /// Set the plugin log file path. Events will be appended here in addition to tracing.
    pub fn with_log_file(mut self, path: PathBuf) -> Self {
        self.log_path = Some(path);
        self
    }

    /// Append a line to the plugin log file (best-effort, never fails).
    fn log_to_file(&self, level: &str, message: &str) {
        if let Some(ref path) = self.log_path
            && let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
        {
            let now = chrono::Utc::now().format("%H:%M:%S");
            let _ = writeln!(f, "[{now}] {level}: {message}");
        }
    }

    /// Check if the plugin's config.json changed and reload tools if so.
    fn maybe_reload_tools(&mut self, agent: &mut AgentLoop) {
        let config_path = LukanPaths::plugin_config(&self.name);
        let current_mtime = std::fs::metadata(&config_path)
            .and_then(|m| m.modified())
            .ok();

        if current_mtime == self.config_mtime {
            return;
        }

        self.config_mtime = current_mtime;

        let tools_list: Vec<String> = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .and_then(|config| {
                config.get("tools").and_then(|v| v.as_array()).map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
            })
            .unwrap_or_else(|| self.default_tools.clone());

        let mut registry = lukan_tools::create_default_registry();
        let refs: Vec<&str> = tools_list.iter().map(|s| s.as_str()).collect();
        registry.retain(&refs);

        agent.reload_tools(registry);
        self.log_to_file(
            "INFO",
            &format!("Tools reloaded: {}", tools_list.join(", ")),
        );
        info!(plugin = %self.name, "Tools hot-reloaded: {}", tools_list.join(", "));
    }

    /// Main loop: reads PluginMessages, processes them, sends HostMessages back.
    pub async fn run(
        &mut self,
        agent: &mut AgentLoop,
        mut plugin_rx: mpsc::Receiver<PluginMessage>,
        host_tx: mpsc::Sender<HostMessage>,
    ) -> Result<()> {
        let mut processing: HashSet<String> = HashSet::new();

        while let Some(msg) = plugin_rx.recv().await {
            // Check for config changes before processing each message
            self.maybe_reload_tools(agent);

            match msg {
                PluginMessage::ChannelMessage {
                    request_id,
                    sender,
                    channel_id,
                    content,
                } => {
                    // Deduplicate by channel_id
                    if processing.contains(&channel_id) {
                        info!(
                            plugin = %self.name,
                            channel_id = %channel_id,
                            "Already processing, skipping"
                        );
                        continue;
                    }
                    processing.insert(channel_id.clone());

                    let preview = if content.len() > 80 {
                        format!("{}...", &content[..80])
                    } else {
                        content.clone()
                    };
                    self.log_to_file("MSG", &format!("{sender} → {preview}"));
                    info!(
                        plugin = %self.name,
                        sender = %sender,
                        channel_id = %channel_id,
                        "Processing message"
                    );

                    let response =
                        collect_agent_response(agent, &content, self.max_response_len).await;
                    let is_error = response.starts_with("Error:");

                    let resp_preview = if response.len() > 120 {
                        format!("{}...", &response[..120])
                    } else {
                        response.clone()
                    };
                    self.log_to_file("REPLY", &format!("→ {channel_id}: {resp_preview}"));

                    let reply = HostMessage::AgentResponse {
                        request_id,
                        text: response,
                        is_error,
                    };

                    if let Err(e) = host_tx.send(reply).await {
                        self.log_to_file("ERROR", &format!("Failed to send response: {e}"));
                        error!(plugin = %self.name, "Failed to send agent response: {e}");
                    }

                    processing.remove(&channel_id);
                }
                PluginMessage::Status { status } => {
                    let status_str = match status {
                        PluginStatus::Connected => "connected",
                        PluginStatus::Disconnected => "disconnected",
                        PluginStatus::Reconnecting => "reconnecting",
                        PluginStatus::Authenticating => "authenticating",
                    };
                    self.log_to_file("STATUS", status_str);
                    info!(plugin = %self.name, status = %status_str, "Plugin status update");
                }
                PluginMessage::Log { level, message } => {
                    let level_str = match level {
                        LogLevel::Debug => "DEBUG",
                        LogLevel::Info => "INFO",
                        LogLevel::Warn => "WARN",
                        LogLevel::Error => "ERROR",
                    };
                    self.log_to_file(level_str, &message);
                    match level {
                        LogLevel::Debug => tracing::debug!(plugin = %self.name, "{message}"),
                        LogLevel::Info => info!(plugin = %self.name, "{message}"),
                        LogLevel::Warn => warn!(plugin = %self.name, "{message}"),
                        LogLevel::Error => error!(plugin = %self.name, "{message}"),
                    }
                }
                PluginMessage::Error {
                    message,
                    recoverable,
                } => {
                    self.log_to_file("ERROR", &format!("{message} (recoverable={recoverable})"));
                    error!(
                        plugin = %self.name,
                        recoverable,
                        "Plugin error: {message}"
                    );
                    if !recoverable {
                        warn!(plugin = %self.name, "Non-recoverable error, stopping channel loop");
                        break;
                    }
                }
                PluginMessage::Ready {
                    version,
                    capabilities,
                } => {
                    self.log_to_file("INFO", &format!("Ready v{version}"));
                    info!(
                        plugin = %self.name,
                        version = %version,
                        ?capabilities,
                        "Plugin sent Ready (unexpected in channel loop)"
                    );
                }
            }
        }

        self.log_to_file("INFO", "Channel loop ended");
        info!(plugin = %self.name, "Plugin channel loop ended");
        Ok(())
    }
}

/// Run the agent for a single message and collect the text response.
/// Resets accumulated text on each ToolResult (only keeps final text).
/// Caps at `max_len` characters.
async fn collect_agent_response(agent: &mut AgentLoop, message: &str, max_len: usize) -> String {
    let (event_tx, mut event_rx) = mpsc::channel::<StreamEvent>(256);

    let turn_result = agent.run_turn(message, event_tx).await;

    if let Err(e) = turn_result {
        error!("Agent turn error: {e}");
        return format!("Error: {e}");
    }

    let mut response = String::new();

    while let Ok(event) = event_rx.try_recv() {
        match event {
            StreamEvent::TextDelta { text } => {
                response.push_str(&text);
            }
            StreamEvent::ToolResult { .. } => {
                // Reset — only keep text after the last tool result
                response.clear();
            }
            _ => {}
        }
    }

    // Truncate if too long
    if response.len() > max_len {
        response.truncate(max_len);
        response.push_str("... (truncated)");
    }

    if response.is_empty() {
        "(No response)".to_string()
    } else {
        response
    }
}
