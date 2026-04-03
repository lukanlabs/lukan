use super::helpers::format_tool_result;
use super::*;

impl App {
    /// Create a new Event Agent — autonomous sub-agent for investigating system events.
    /// Uses PermissionMode::Skip (no approval prompts) and a specialized system prompt.
    pub(super) async fn create_event_agent(&self) -> AgentLoop {
        const EVENT_AGENT_PROMPT: &str = include_str!("../../../../prompts/event-agent.txt");

        let system_prompt = SystemPrompt::Text(EVENT_AGENT_PROMPT.to_string());
        let cwd = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());

        let project_cfg = lukan_core::config::ProjectConfig::load(&cwd)
            .await
            .ok()
            .flatten()
            .map(|(_, cfg)| cfg);

        let permissions = project_cfg
            .as_ref()
            .map(|c| c.permissions.clone())
            .unwrap_or_default();

        let allowed = project_cfg
            .as_ref()
            .map(|c| c.resolve_allowed_paths(&cwd))
            .unwrap_or_else(|| vec![cwd.clone()]);

        let tools = create_configured_registry(&permissions, &allowed);

        let config = AgentConfig {
            provider: Arc::clone(&self.provider),
            tools,
            system_prompt,
            cwd,
            provider_name: self.config.config.provider.to_string(),
            model_name: self.config.effective_model().unwrap_or_default(),
            bg_signal: None,
            allowed_paths: Some(allowed),
            permission_mode: PermissionMode::Skip,
            permission_mode_rx: None,
            permissions,
            approval_rx: None,
            plan_review_rx: None,
            planner_answer_rx: None,
            browser_tools: false,
            skip_session_save: false,
            vision_provider: None,
            extra_env: self.config.credentials.flatten_skill_env(),
            compaction_threshold: None,
        };

        match AgentLoop::new(config).await {
            Ok(mut agent) => {
                agent.set_disabled_tools(self.disabled_tools.clone());
                agent
            }
            Err(e) => panic!("Failed to create event agent: {e}"),
        }
    }

    /// Poll `pending.jsonl` and route events to the Event Agent instead of the main agent.
    /// Returns `true` if any `error` or `critical` events were found.
    pub(super) fn poll_pending_events_to_event_agent(&mut self) -> bool {
        let path = LukanPaths::pending_events_file();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) if !c.trim().is_empty() => c,
            _ => return false,
        };
        // Truncate immediately so we don't re-read the same events
        let _ = std::fs::write(&path, "");

        // Append raw lines to history.jsonl (persistent log)
        let history_path = LukanPaths::events_history_file();
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&history_path)
        {
            use std::io::Write;
            for line in content.lines() {
                if !line.trim().is_empty() {
                    let _ = writeln!(f, "{}", line.trim());
                }
            }
        }
        // Auto-rotate: keep last 200 events
        Self::rotate_event_history(&history_path, 200);

        let mut has_critical = false;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                let source = val
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                let level = val
                    .get("level")
                    .and_then(|v| v.as_str())
                    .unwrap_or("info")
                    .to_string();
                let detail = val
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let msg = format!("[{}] {}: {}", level.to_uppercase(), source, detail);
                self.toast_notifications.push((msg, Instant::now()));
                if level == "error" || level == "critical" {
                    has_critical = true;
                }
                // Buffer events for the Event Agent (not the main agent)
                self.event_buffer.push((source, level, detail));
            }
        }
        has_critical
    }

    /// Rotate history file: if it exceeds `max_lines`, trim to the last `max_lines / 2`.
    pub(super) fn rotate_event_history(path: &std::path::Path, max_lines: usize) {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return,
        };
        let lines: Vec<&str> = content.lines().collect();
        if lines.len() <= max_lines {
            return;
        }
        // Keep last half
        let keep = max_lines / 2;
        let trimmed: Vec<&str> = lines[lines.len() - keep..].to_vec();
        let _ = std::fs::write(path, trimmed.join("\n") + "\n");
    }

    /// Load the last `n` events from history.jsonl, newest first.
    pub(super) fn load_event_history(n: usize) -> Vec<(String, String, String, String)> {
        let path = LukanPaths::events_history_file();
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut events: Vec<(String, String, String, String)> = vec![];
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                let ts = val
                    .get("ts")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let level = val
                    .get("level")
                    .and_then(|v| v.as_str())
                    .unwrap_or("info")
                    .to_string();
                let source = val
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string();
                let detail = val
                    .get("detail")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                events.push((ts, level, source, detail));
            }
        }
        // Return last n, newest first
        events.reverse();
        events.truncate(n);
        events
    }

    /// Flush buffered system events into the Event Agent's context as messages.
    pub(super) fn flush_event_buffer_to_event_agent(&mut self) {
        if self.event_buffer.is_empty() {
            return;
        }
        if let Some(ref mut agent) = self.event_agent {
            for (source, level, detail) in self.event_buffer.drain(..) {
                agent.push_event(&source, &level, &detail);
            }
        }
        // If event_agent doesn't exist yet, events stay in buffer until it's created
    }

    /// Trigger an autonomous turn of the Event Agent to investigate critical events.
    /// Creates the agent lazily if it doesn't exist.
    pub(super) async fn trigger_event_agent_auto_turn(
        &mut self,
        event_agent_tx: mpsc::Sender<StreamEvent>,
    ) {
        // Don't trigger if already streaming
        if self.event_is_streaming {
            return;
        }

        // Create event agent lazily
        if self.event_agent.is_none() {
            let agent = self.create_event_agent().await;
            self.event_agent = Some(agent);
        }
        if let Some(ref mut agent) = self.event_agent {
            agent.set_disabled_tools(self.disabled_tools.clone());
        }

        // Flush buffered events into the agent
        self.flush_event_buffer_to_event_agent();

        // Take the agent for the turn
        let agent = match self.event_agent.take() {
            Some(a) => a,
            None => return,
        };

        // Add synthetic message in the event view
        self.event_messages.push(ChatMessage::new(
            "system",
            "Investigating system events...".to_string(),
        ));

        self.event_is_streaming = true;
        self.event_streaming_text.clear();
        self.event_active_tool = None;

        // Mark unread if user is not watching
        if self.active_view != ActiveView::EventAgent {
            self.event_agent_has_unread = true;
        }

        let (return_tx, return_rx) = tokio::sync::oneshot::channel::<AgentLoop>();
        self.event_agent_return_rx = Some(return_rx);

        let mut agent = agent;
        tokio::spawn(async move {
            let prompt = "New system events have arrived. Analyze them, investigate the root cause, \
                 and take simple corrective action if safe. Report your findings.";
            if let Err(e) = agent
                .run_turn(prompt, event_agent_tx.clone(), None, None)
                .await
            {
                error!("Event agent error: {e}");
                let _ = event_agent_tx
                    .send(StreamEvent::Error {
                        error: e.to_string(),
                    })
                    .await;
            }
            let _ = return_tx.send(agent);
        });
    }

    /// Handle stream events from the Event Agent (mirror of handle_stream_event).
    /// Simplified: no ApprovalRequired, PlanReview, etc. (Event Agent uses Skip mode).
    pub(super) fn handle_event_agent_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::MessageStart => {
                self.event_streaming_text.clear();
                self.event_active_tool = None;
            }
            StreamEvent::TextDelta { text } => {
                self.event_streaming_text.push_str(&text);
            }
            StreamEvent::ToolUseStart { name, .. } => {
                self.event_active_tool = Some(name.clone());
                let content = std::mem::take(&mut self.event_streaming_text);
                if !content.trim().is_empty() {
                    self.event_messages
                        .push(ChatMessage::new("assistant", content.trim_end()));
                }
            }
            StreamEvent::ToolUseEnd { id, name, input } => {
                let summary = summarize_tool_input(&name, &input);
                let mut msg = ChatMessage::new("tool_call", format!("● {name}({summary})"));
                msg.tool_id = Some(id);
                self.event_messages.push(msg);
            }
            StreamEvent::ToolProgress { id, content, .. } => {
                let insert_pos = self.event_tool_insert_position(&id);
                let mut msg = ChatMessage::new("tool_result", format!("  ⎿  {content}"));
                msg.tool_id = Some(id);
                self.event_messages.insert(insert_pos, msg);
            }
            StreamEvent::ToolResult {
                id,
                content,
                is_error,
                ..
            } => {
                self.event_active_tool = None;
                let formatted = format_tool_result(&content, is_error.unwrap_or(false));
                let insert_pos = self.event_tool_insert_position(&id);
                let mut msg = ChatMessage::new("tool_result", formatted);
                msg.tool_id = Some(id);
                self.event_messages.insert(insert_pos, msg);
            }
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
                ..
            } => {
                self.event_input_tokens += input_tokens;
                self.event_output_tokens += output_tokens;
            }
            StreamEvent::MessageEnd { .. } => {
                let content = std::mem::take(&mut self.event_streaming_text);
                if !content.trim().is_empty() {
                    self.event_messages
                        .push(ChatMessage::new("assistant", content.trim_end()));
                }
                self.event_is_streaming = false;
            }
            StreamEvent::Error { error } => {
                self.event_messages
                    .push(ChatMessage::new("assistant", format!("Error: {error}")));
                self.event_is_streaming = false;
            }
            _ => {}
        }
    }

    /// Find tool insert position in event_messages (mirror of tool_insert_position).
    fn event_tool_insert_position(&self, tool_id: &str) -> usize {
        let call_idx = self
            .event_messages
            .iter()
            .rposition(|m| m.role == "tool_call" && m.tool_id.as_deref() == Some(tool_id));
        match call_idx {
            Some(idx) => {
                let mut pos = idx + 1;
                while pos < self.event_messages.len()
                    && self.event_messages[pos].tool_id.as_deref() == Some(tool_id)
                {
                    pos += 1;
                }
                pos
            }
            None => self.event_messages.len(),
        }
    }

    /// Submit a user message to the Event Agent (when in EventAgent view).
    pub(super) async fn submit_to_event_agent(
        &mut self,
        event_agent_tx: mpsc::Sender<StreamEvent>,
    ) {
        if self.event_is_streaming {
            // Event agent is busy — ignore input
            return;
        }

        let text = self.input.trim().to_string();
        let display = self.display_input().trim().to_string();
        self.input.clear();
        self.cursor_pos = 0;
        self.paste_info = None;

        if text.is_empty() {
            return;
        }

        self.event_messages.push(ChatMessage::new("user", display));

        // Create event agent lazily
        if self.event_agent.is_none() {
            let agent = self.create_event_agent().await;
            self.event_agent = Some(agent);
        }
        if let Some(ref mut agent) = self.event_agent {
            agent.set_disabled_tools(self.disabled_tools.clone());
        }

        // Flush any buffered events
        self.flush_event_buffer_to_event_agent();

        let agent = match self.event_agent.take() {
            Some(a) => a,
            None => return,
        };

        self.event_is_streaming = true;
        self.event_streaming_text.clear();
        self.event_active_tool = None;

        let (return_tx, return_rx) = tokio::sync::oneshot::channel::<AgentLoop>();
        self.event_agent_return_rx = Some(return_rx);

        let mut agent = agent;
        tokio::spawn(async move {
            if let Err(e) = agent
                .run_turn(&text, event_agent_tx.clone(), None, None)
                .await
            {
                error!("Event agent error: {e}");
                let _ = event_agent_tx
                    .send(StreamEvent::Error {
                        error: e.to_string(),
                    })
                    .await;
            }
            let _ = return_tx.send(agent);
        });
    }
}
