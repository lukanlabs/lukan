use super::helpers::{build_system_prompt_with_opts, format_tool_result};
use super::*;

impl App {
    /// Open the interactive session picker
    pub(super) async fn open_session_picker(&mut self) {
        let sessions = match SessionManager::list().await {
            Ok(s) => s,
            Err(e) => {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Failed to load sessions: {e}"),
                ));
                return;
            }
        };

        if sessions.is_empty() {
            self.messages
                .push(ChatMessage::new("system", "No saved sessions."));
            return;
        }

        // Pre-select the current session
        let current_id = self.session_id.clone();
        let selected = current_id
            .as_ref()
            .and_then(|id| sessions.iter().position(|s| s.id == *id))
            .unwrap_or(0);

        self.session_picker = Some(SessionPicker {
            sessions,
            selected,
            current_id,
        });
    }

    /// Load the selected session from the picker
    pub(super) async fn load_selected_session(&mut self, idx: usize) {
        let session_id = {
            let picker = self.session_picker.as_ref().unwrap();
            picker.sessions[idx].id.clone()
        };

        // Don't reload the current session
        if self.session_id.as_deref() == Some(&session_id) {
            self.messages
                .push(ChatMessage::new("system", "Already in this session."));
            return;
        }

        // ── Daemon mode: send LoadSession to daemon ──
        if let Some(ref daemon) = self.daemon_tx {
            let msg = crate::ws_client::OutMessage::LoadSession {
                session_id: self.daemon_tab_id.clone(),
                id: Some(session_id.clone()),
            };
            if let Err(e) = daemon.send(&msg) {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Failed to load session: {e}"),
                ));
            } else {
                self.session_id = Some(session_id.clone());
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Loaded session {session_id}"),
                ));
            }
            return;
        }

        // ── In-process mode ──
        let system_prompt = build_system_prompt_with_opts(self.browser_tools).await;
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

        // Create approval channel for loaded session
        let (approval_tx, approval_rx) = mpsc::channel::<ApprovalResponse>(1);
        self.approval_tx = Some(approval_tx);

        // Create plan review channel
        let (plan_review_tx, plan_review_rx) = mpsc::channel::<PlanReviewResponse>(1);
        self.plan_review_tx = Some(plan_review_tx);

        // Create planner answer channel
        let (planner_answer_tx, planner_answer_rx) = mpsc::channel::<String>(1);
        self.planner_answer_tx = Some(planner_answer_tx);

        let mut tools = if self.browser_tools {
            create_configured_browser_registry(&permissions, &allowed)
        } else {
            create_configured_registry(&permissions, &allowed)
        };

        // Register MCP tools if configured
        if !self.config.config.mcp_servers.is_empty() {
            let result =
                lukan_tools::init_mcp_tools(&mut tools, &self.config.config.mcp_servers).await;
            if result.tool_count > 0 {
                tracing::info!(
                    count = result.tool_count,
                    "MCP tools registered (session restore)"
                );
            }
            for (server, err) in &result.errors {
                tracing::warn!(server = %server, "MCP error: {err}");
            }
            self.mcp_manager = Some(result.manager);
        }

        let config = AgentConfig {
            provider: Arc::clone(&self.provider),
            tools,
            system_prompt,
            cwd,
            provider_name: self.config.config.provider.to_string(),
            model_name: self.config.effective_model().unwrap_or_default(),
            bg_signal: Some(self.bg_signal_rx.clone()),
            allowed_paths: Some(allowed),
            permission_mode: self.permission_mode.clone(),
            permission_mode_rx: None,
            permissions,
            approval_rx: Some(approval_rx),
            plan_review_rx: Some(plan_review_rx),
            planner_answer_rx: Some(planner_answer_rx),
            browser_tools: self.browser_tools,
            skip_session_save: false,
            vision_provider: lukan_providers::create_vision_provider(
                self.config.config.vision_model.as_deref(),
                &self.config.credentials,
            )
            .map(std::sync::Arc::from),
            extra_env: self.config.credentials.flatten_skill_env(),
        };

        match AgentLoop::load_session(config, &session_id).await {
            Ok(mut agent) => {
                agent.set_disabled_tools(self.disabled_tools.clone());
                // Rebuild UI messages from the loaded session
                self.messages.clear();
                self.committed_msg_idx = 0;
                self.viewport_scroll = 0;

                // Reconstruct chat messages from agent history
                let session = SessionManager::load(&session_id).await.ok().flatten();
                if let Some(session) = session {
                    use lukan_core::models::messages::{ContentBlock, MessageContent, Role};

                    // First pass: collect tool results by tool_use_id
                    let mut tool_results: HashMap<String, (String, bool, Option<String>)> =
                        HashMap::new();
                    for msg in &session.messages {
                        if let MessageContent::Blocks(blocks) = &msg.content {
                            for block in blocks {
                                if let ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                    is_error,
                                    diff,
                                    ..
                                } = block
                                {
                                    tool_results.insert(
                                        tool_use_id.clone(),
                                        (content.clone(), is_error.unwrap_or(false), diff.clone()),
                                    );
                                }
                            }
                        }
                    }

                    // Second pass: reconstruct UI messages
                    for msg in &session.messages {
                        match msg.role {
                            Role::User => {
                                // Only show user messages that have text (skip tool-result-only messages)
                                let text = match &msg.content {
                                    MessageContent::Text(s) => Some(s.clone()),
                                    MessageContent::Blocks(blocks) => {
                                        let texts: Vec<&str> = blocks
                                            .iter()
                                            .filter_map(|b| {
                                                if let ContentBlock::Text { text } = b {
                                                    Some(text.as_str())
                                                } else {
                                                    None
                                                }
                                            })
                                            .collect();
                                        if texts.is_empty() {
                                            None
                                        } else {
                                            Some(texts.join("\n"))
                                        }
                                    }
                                };
                                if let Some(text) = text
                                    && !text.is_empty()
                                {
                                    self.messages.push(ChatMessage::new("user", text));
                                }
                            }
                            Role::Assistant => match &msg.content {
                                MessageContent::Text(text) => {
                                    if !text.is_empty() {
                                        self.messages
                                            .push(ChatMessage::new("assistant", text.clone()));
                                    }
                                }
                                MessageContent::Blocks(blocks) => {
                                    // Collect text blocks
                                    let text: String = blocks
                                        .iter()
                                        .filter_map(|b| {
                                            if let ContentBlock::Text { text } = b {
                                                Some(text.as_str())
                                            } else {
                                                None
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n");

                                    if !text.is_empty() {
                                        self.messages.push(ChatMessage::new("assistant", text));
                                    }

                                    // Render tool uses with their results
                                    for block in blocks {
                                        if let ContentBlock::ToolUse { id, name, input } = block {
                                            let summary = summarize_tool_input(name, input);
                                            self.messages.push(ChatMessage::new(
                                                "tool_call",
                                                format!("● {name}({summary})"),
                                            ));

                                            if let Some((content, is_error, diff)) =
                                                tool_results.get(id)
                                            {
                                                let formatted =
                                                    format_tool_result(content, *is_error);
                                                self.messages.push(ChatMessage::with_diff(
                                                    "tool_result",
                                                    formatted,
                                                    diff.clone(),
                                                ));
                                            }
                                        }
                                    }
                                }
                            },
                            _ => {}
                        }
                    }
                }

                self.input_tokens = agent.input_tokens();
                self.output_tokens = agent.output_tokens();
                self.context_size = agent.last_context_size();
                self.session_id = Some(agent.session_id().to_string());
                self.agent = Some(agent);

                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Loaded session {session_id}"),
                ));
            }
            Err(e) => {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Failed to load session: {e}"),
                ));
            }
        }
    }

    /// Restore to a checkpoint, truncating agent history and optionally reverting files
    pub(super) async fn restore_to_checkpoint(&mut self, checkpoint_id: &str, restore_code: bool) {
        // Daemon mode: send to daemon
        if let Some(ref daemon) = self.daemon_tx {
            let _ = daemon.send(&crate::ws_client::OutMessage::RestoreCheckpoint {
                checkpoint_id: checkpoint_id.to_string(),
                restore_code,
                session_id: self.daemon_tab_id.clone(),
            });
            return;
        }

        let agent = match self.agent.as_mut() {
            Some(a) => a,
            None => {
                self.messages
                    .push(ChatMessage::new("system", "No active session to restore."));
                return;
            }
        };

        match agent.restore_checkpoint(checkpoint_id, restore_code).await {
            Ok(_) => {
                let msg = if restore_code {
                    format!("Restored to checkpoint {checkpoint_id} (code reverted)")
                } else {
                    format!("Restored to checkpoint {checkpoint_id} (history only)")
                };
                self.messages.push(ChatMessage::new("system", msg));
                // Resync token counts after history truncation
                self.input_tokens = agent.input_tokens();
                self.output_tokens = agent.output_tokens();
                self.context_size = agent.last_context_size();
            }
            Err(e) => {
                self.messages
                    .push(ChatMessage::new("system", format!("Restore failed: {e}")));
            }
        }
    }
}
