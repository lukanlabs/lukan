use super::helpers::{format_tool_progress_named, format_tool_result_named};
use super::*;

impl App {
    fn build_subagent_completion_message(update: &SubAgentUpdate) -> String {
        let status = match update.status.as_str() {
            "completed" => "completed",
            "error" => "error",
            "aborted" => "aborted",
            other => other,
        };

        let task_preview = if update.task.len() > 50 {
            format!("{}...", &update.task[..update.task.floor_char_boundary(47)])
        } else {
            update.task.trim().to_string()
        };

        format!("SubAgent {} {}: {}", update.id, status, task_preview)
    }

    fn maybe_forward_subagent_completion(&mut self, update: &SubAgentUpdate) {
        if !matches!(update.status.as_str(), "completed" | "error" | "aborted") {
            return;
        }

        if update.tab_id.as_deref() != self.daemon_tab_id.as_deref() && update.tab_id.is_some() {
            return;
        }

        let message = Self::build_subagent_completion_message(update);

        if let Some(ref daemon) = self.daemon_tx {
            let _ = daemon.send(&crate::ws_client::OutMessage::QueueMessage {
                content: message.clone(),
                display_content: Some(message.clone()),
                session_id: self.daemon_tab_id.clone(),
            });
        } else {
            self.queued_messages.lock().unwrap().push(message.clone());
            if !self.is_streaming {
                self.messages.push(ChatMessage::new("user", &message));
                self.input.clear();
                self.cursor_pos = 0;
                self.pending_queue_submit = true;
            }
        }
    }

    fn maybe_forward_bash_completion(&mut self, pid: u32, text: &str, display_text: Option<&str>, tab_id: Option<&str>) {
        if tab_id != self.daemon_tab_id.as_deref() && tab_id.is_some() {
            return;
        }

        let visible = display_text.unwrap_or(text).to_string();

        if let Some(ref daemon) = self.daemon_tx {
            let _ = daemon.send(&crate::ws_client::OutMessage::QueueMessage {
                content: text.to_string(),
                display_content: Some(visible.clone()),
                session_id: self.daemon_tab_id.clone(),
            });
        } else {
            self.queued_messages.lock().unwrap().push(text.to_string());
            if !self.is_streaming {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Background Bash process completed. PID: {pid}."),
                ));
                self.messages.push(ChatMessage::new("user", &visible));
                self.input.clear();
                self.cursor_pos = 0;
                self.pending_queue_submit = true;
            }
        }
    }

    pub(super) fn handle_subagent_update(&mut self, update: SubAgentUpdate) {
        // Upsert into global manager so Alt+S can find daemon subagents
        let update_for_upsert = update.clone();
        tokio::spawn(async move {
            lukan_agent::sub_agent::upsert_from_update(&update_for_upsert).await;
        });

        // Show message when subagent completes or errors
        if update.status == "completed" || update.status == "error" || update.status == "aborted" {
            let task_preview = if update.task.len() > 50 {
                format!("{}...", &update.task[..update.task.floor_char_boundary(47)])
            } else {
                update.task.clone()
            };
            self.messages.push(ChatMessage::new(
                "system",
                format!("SubAgent {} {}: {}", update.id, update.status, task_preview),
            ));
            self.maybe_forward_subagent_completion(&update);
        }

        if let Some(ref mut picker) = self.subagent_picker {
            // Update detail view if viewing this specific agent
            if picker.view == SubAgentPickerView::ChatDetail && picker.detail_id == update.id {
                picker.detail_status = update.status.clone();
                picker.detail_turns = format!("{}", update.turns);
                picker.detail_tokens = format!(
                    "{}in/{}out tokens",
                    update.input_tokens, update.output_tokens
                );
                picker.detail_error = update.error.clone();
                picker.detail_messages = update
                    .chat_messages
                    .iter()
                    .map(|m| ChatMessage::new(&m.role, &m.content))
                    .collect();
            }
        }
    }

    /// Send an approval response — routes to daemon or in-process channel.
    pub(super) fn send_approval(&self, response: ApprovalResponse) {
        if let Some(ref daemon) = self.daemon_tx {
            let tab = self.daemon_tab_id.clone();
            let msg = match &response {
                ApprovalResponse::Approved { approved_ids } => {
                    crate::ws_client::OutMessage::Approve {
                        approved_ids: approved_ids.clone(),
                        session_id: tab,
                    }
                }
                ApprovalResponse::AlwaysAllow {
                    approved_ids,
                    tools,
                } => crate::ws_client::OutMessage::AlwaysAllow {
                    approved_ids: approved_ids.clone(),
                    tools: tools.clone(),
                    session_id: tab,
                },
                ApprovalResponse::DeniedAll => {
                    crate::ws_client::OutMessage::DenyAll { session_id: tab }
                }
            };
            let _ = daemon.send(&msg);
        } else if let Some(ref tx) = self.approval_tx {
            let _ = tx.try_send(response);
        }
    }

    /// Send a plan review response — routes to daemon or in-process channel.
    pub(super) fn send_plan_review(&self, response: PlanReviewResponse) {
        if let Some(ref daemon) = self.daemon_tx {
            let tab = self.daemon_tab_id.clone();
            let msg = match &response {
                PlanReviewResponse::Accepted { modified_tasks } => {
                    crate::ws_client::OutMessage::PlanAccept {
                        tasks: modified_tasks
                            .as_ref()
                            .and_then(|t| serde_json::to_value(t).ok()),
                        session_id: tab,
                    }
                }
                PlanReviewResponse::Rejected { feedback } => {
                    crate::ws_client::OutMessage::PlanReject {
                        feedback: feedback.clone(),
                        session_id: tab,
                    }
                }
                PlanReviewResponse::TaskFeedback {
                    task_index,
                    feedback,
                } => crate::ws_client::OutMessage::PlanTaskFeedback {
                    task_index: *task_index as u32,
                    feedback: feedback.clone(),
                    session_id: tab,
                },
            };
            let _ = daemon.send(&msg);
        } else if let Some(ref tx) = self.plan_review_tx {
            let _ = tx.try_send(response);
        }
    }

    /// Send a planner answer — routes to daemon or in-process channel.
    pub(super) fn send_planner_answer(&self, answer: String) {
        if let Some(ref daemon) = self.daemon_tx {
            let msg = crate::ws_client::OutMessage::AnswerQuestion {
                answer,
                session_id: self.daemon_tab_id.clone(),
            };
            let _ = daemon.send(&msg);
        } else if let Some(ref tx) = self.planner_answer_tx {
            let _ = tx.try_send(answer);
        }
    }

    /// Handle events from the daemon WebSocket connection.
    pub(super) fn handle_daemon_event(&mut self, event: crate::ws_client::DaemonEvent) {
        use crate::ws_client::DaemonEvent;
        match event {
            DaemonEvent::Stream(stream_event, saved_session_id) => {
                // Broadcasts from other clients carry savedSessionId.
                // Accept if it matches our session (same session in Web UI),
                // reject if it's a different session.
                if let Some(ref broadcast_sid) = saved_session_id {
                    match &self.session_id {
                        Some(our_sid) if broadcast_sid == our_sid => {
                            // Same session — accept (e.g. Web UI talking in our session)
                        }
                        _ => return, // Different session or no session yet — ignore
                    }
                }
                self.handle_stream_event(stream_event);
            }
            DaemonEvent::UserMessage {
                content,
                saved_session_id,
            } => {
                // Show user messages from other clients in the same session
                if let Some(ref broadcast_sid) = saved_session_id
                    && self.session_id.as_deref() == Some(broadcast_sid)
                {
                    self.messages.push(ChatMessage::new("user", &content));
                }
            }
            DaemonEvent::Init { session_id, .. } => {
                self.session_id = Some(session_id);
            }
            DaemonEvent::TabCreated { .. } => {}
            DaemonEvent::ProcessingComplete {
                session_id,
                context_size,
                aborted: _,
            } => {
                self.session_id = Some(session_id);
                if let Some(cs) = context_size {
                    self.context_size = cs;
                }
                // Ensure streaming is stopped (handle_stream_event should have done this
                // via MessageEnd, but this is a safety net)
                if self.is_streaming {
                    self.is_streaming = false;
                }
                // Now that the daemon is done processing, submit any queued messages
                if !self.queued_messages.lock().unwrap().is_empty() {
                    let remaining: Vec<String> =
                        self.queued_messages.lock().unwrap().drain(..).collect();
                    self.input = remaining.join("\n");
                    self.cursor_pos = self.input.len();
                    self.pending_queue_submit = true;
                }
            }
            DaemonEvent::SessionList { sessions } => {
                self.session_picker = Some(SessionPicker {
                    sessions: sessions
                        .into_iter()
                        .map(|s| SessionSummary {
                            id: s.id,
                            name: s.name,
                            created_at: s.created_at,
                            updated_at: s.updated_at,
                            message_count: s.message_count,
                            provider: s.provider,
                            model: s.model,
                            last_message: s.last_message,
                            cwd: None,
                            project_root: None,
                        })
                        .collect(),
                    selected: 0,
                    current_id: self.session_id.clone(),
                });
                self.force_redraw = true;
            }
            DaemonEvent::SessionLoaded {
                session_id,
                messages: loaded_messages,
                context_size,
            } => {
                self.session_id = Some(session_id.clone());
                self.context_size = context_size;
                // Clear current messages and populate from loaded session
                self.messages.clear();
                self.committed_msg_idx = 0;
                self.viewport_scroll = 0;
                for msg in &loaded_messages {
                    use lukan_core::models::messages::{ContentBlock, MessageContent, Role};
                    let display_role = match msg.role {
                        Role::User => "user",
                        Role::Assistant => "assistant",
                        _ => "system",
                    };
                    let blocks = match &msg.content {
                        MessageContent::Text(text) => {
                            if !text.trim().is_empty() {
                                self.messages.push(ChatMessage::new(display_role, text));
                            }
                            continue;
                        }
                        MessageContent::Blocks(blocks) => blocks,
                    };
                    for block in blocks {
                        match block {
                            ContentBlock::Text { text } if !text.trim().is_empty() => {
                                self.messages.push(ChatMessage::new(display_role, text));
                            }
                            ContentBlock::ToolUse { name, input, .. } => {
                                let summary = summarize_tool_input(name, input);
                                self.messages.push(ChatMessage::new(
                                    "tool_call",
                                    format!("● {name}({summary})"),
                                ));
                            }
                            ContentBlock::ToolResult { content, .. } => {
                                let preview = if content.len() > 200 {
                                    let end = content.floor_char_boundary(200);
                                    format!("{}...", &content[..end])
                                } else {
                                    content.clone()
                                };
                                self.messages.push(ChatMessage::new("tool_result", preview));
                            }
                            _ => {}
                        }
                    }
                }
                self.messages.push(ChatMessage::new(
                    "system",
                    format!(
                        "Loaded session {session_id} ({} messages)",
                        loaded_messages.len()
                    ),
                ));
                self.force_redraw = true;
            }
            DaemonEvent::ModelChanged {
                provider_name,
                model_name,
            } => {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Model changed to {provider_name}:{model_name}"),
                ));
            }
            DaemonEvent::CheckpointList { checkpoints } => {
                if checkpoints.is_empty() {
                    self.messages.push(ChatMessage::new(
                        "system",
                        "No checkpoints in current session.",
                    ));
                } else {
                    let entries: Vec<RewindEntry> = checkpoints
                        .iter()
                        .map(|cp| {
                            let (additions, deletions) =
                                cp.snapshots.iter().fold((0u32, 0u32), |(a, d), snap| {
                                    (
                                        a + snap
                                            .diff
                                            .as_ref()
                                            .map(|d| d.matches("\n+").count() as u32)
                                            .unwrap_or(0),
                                        d + snap
                                            .diff
                                            .as_ref()
                                            .map(|d| d.matches("\n-").count() as u32)
                                            .unwrap_or(0),
                                    )
                                });
                            RewindEntry {
                                checkpoint_id: Some(cp.id.clone()),
                                message: cp.message.clone(),
                                files_changed: cp.snapshots.len(),
                                additions,
                                deletions,
                            }
                        })
                        .collect();
                    self.rewind_picker = Some(RewindPicker::new(entries));
                }
                self.force_redraw = true;
            }
            DaemonEvent::CompactComplete {
                session_id,
                messages: _,
            } => {
                self.session_id = Some(session_id);
                self.messages.push(ChatMessage::new(
                    "system",
                    "Session compacted successfully.",
                ));
                self.force_redraw = true;
            }
            DaemonEvent::CheckpointRestored {
                session_id,
                messages: _,
            } => {
                self.session_id = Some(session_id);
                self.messages
                    .push(ChatMessage::new("system", "Checkpoint restored."));
                self.force_redraw = true;
            }
            DaemonEvent::BgProcessList { processes } => {
                if processes.is_empty() {
                    self.messages
                        .push(ChatMessage::new("system", "No background processes."));
                } else {
                    let entries: Vec<BgEntry> = processes
                        .into_iter()
                        .map(|p| {
                            let alive = p.status == "Running";
                            let started_at = chrono::DateTime::parse_from_rfc3339(&p.started_at)
                                .map(|dt| dt.with_timezone(&Utc))
                                .unwrap_or_else(|_| Utc::now());
                            BgEntry {
                                pid: p.pid,
                                command: p.command,
                                started_at,
                                alive,
                            }
                        })
                        .collect();
                    self.bg_picker = Some(BgPicker::new(entries));
                }
                self.force_redraw = true;
            }
            DaemonEvent::BgProcessLog { pid, log } => {
                // Update the bg_picker log view if it's showing this process
                if let Some(ref mut picker) = self.bg_picker
                    && (picker.log_pid == pid || picker.log_pid == 0)
                {
                    picker.log_content = log;
                    picker.log_pid = pid;
                }
                self.force_redraw = true;
            }
            DaemonEvent::Error(error) => {
                self.is_streaming = false;
                self.messages
                    .push(ChatMessage::new("system", format!("Error: {error}")));
            }
            DaemonEvent::Disconnected => {
                self.is_streaming = false;
                self.daemon_tx = None;
                self.messages.push(ChatMessage::new(
                    "system",
                    "Disconnected from daemon. Falling back to in-process mode.",
                ));
            }
        }
    }

    pub(super) fn handle_stream_event(&mut self, event: StreamEvent) {
        match event {
            StreamEvent::MessageStart => {
                self.streaming_text.clear();
                self.streaming_thinking.clear();
                self.active_tool = None;
                self.turn_text_msg_idx = None;
            }
            StreamEvent::TextDelta { text } => {
                // Thinking normally ends the moment real text begins — flush
                // the accumulated reasoning into a "thinking" message so it
                // stays visible in scroll-back after the turn.
                if !self.streaming_thinking.is_empty() {
                    let content = std::mem::take(&mut self.streaming_thinking);
                    let trimmed = content.trim().to_string();
                    if !trimmed.is_empty() {
                        self.messages.push(ChatMessage::new("thinking", trimmed));
                    }
                }
                self.streaming_text.push_str(&text);
            }
            StreamEvent::ThinkingDelta { text } => {
                self.streaming_thinking.push_str(&text);
            }
            StreamEvent::ToolUseStart { name, .. } => {
                let is_silent = self.config.config.silent_tools.iter().any(|s| s == &name);
                // Flush thinking first so it sits above any forthcoming
                // assistant text / tool call in the final transcript.
                if !self.streaming_thinking.is_empty() {
                    let content = std::mem::take(&mut self.streaming_thinking);
                    let trimmed = content.trim().to_string();
                    if !trimmed.is_empty() {
                        self.messages.push(ChatMessage::new("thinking", trimmed));
                    }
                }
                // Flush current text as a message before tool call.
                // If we already flushed text earlier in this turn, append to
                // the same message so mid-sentence splits don't occur.
                if !is_silent {
                    self.active_tool = Some(name.clone());
                }
                let content = std::mem::take(&mut self.streaming_text);
                let trimmed = content.trim_end().to_string();
                if !trimmed.is_empty() {
                    if let Some(idx) = self.turn_text_msg_idx {
                        if idx < self.messages.len() && self.messages[idx].role == "assistant" {
                            self.messages[idx].content.push_str(&trimmed);
                        } else {
                            self.messages.push(ChatMessage::new("assistant", trimmed));
                            self.turn_text_msg_idx = Some(self.messages.len() - 1);
                        }
                    } else {
                        self.messages.push(ChatMessage::new("assistant", trimmed));
                        self.turn_text_msg_idx = Some(self.messages.len() - 1);
                    }
                }
            }
            StreamEvent::ToolUseEnd { id, name, input } => {
                let is_silent = self.config.config.silent_tools.iter().any(|s| s == &name);
                if !is_silent {
                    // ● ToolName(input summary)
                    let summary = summarize_tool_input(&name, &input);
                    let mut msg = ChatMessage::new("tool_call", format!("● {name}({summary})"));
                    msg.tool_id = Some(id);
                    self.messages.push(msg);
                }

                // Auto-refresh task panel when task tools are used
                if matches!(name.as_str(), "TaskAdd" | "TaskUpdate" | "TaskList") {
                    self.task_panel_needs_refresh = true;
                }
            }
            StreamEvent::ToolProgress { id, name, content } => {
                let is_silent = self.config.config.silent_tools.iter().any(|s| s == &name);
                if is_silent {
                    return;
                }
                self.active_tool = Some(name.clone());
                let sanitized = sanitize_for_display(&content);
                let insert_pos = self.tool_insert_position(&id);

                // Try to consolidate with existing progress for this tool
                if insert_pos > 0 {
                    let prev = &self.messages[insert_pos - 1];
                    if prev.role == "tool_result"
                        && prev.tool_id.as_deref() == Some(&*id)
                        && prev.diff.is_none()
                    {
                        let prev = &mut self.messages[insert_pos - 1];
                        prev.content.push('\n');
                        prev.content.push_str(&format!("     {sanitized}"));
                        return;
                    }
                }

                let mut msg = ChatMessage::new("tool_result", format_tool_progress_named(&name, &content));
                msg.tool_id = Some(id);
                self.messages.insert(insert_pos, msg);
            }
            StreamEvent::ToolResult {
                id,
                name,
                content,
                is_error,
                diff,
                ..
            } => {
                self.active_tool = None;
                let is_silent = self.config.config.silent_tools.iter().any(|s| s == &name);
                if is_silent {
                    return;
                }
                let is_err = is_error.unwrap_or(false);

                // For compact tools (ReadFile, Grep, Glob): update the existing
                // progress message in-place instead of adding a new line.
                let compact = matches!(name.as_str(), "ReadFiles" | "Grep" | "Glob")
                    && !is_err
                    && diff.is_none();

                if compact {
                    let summary = format_tool_result_named(&name, &content, false);
                    // Find existing progress message for this tool_id and replace
                    if let Some(pos) = self
                        .messages
                        .iter()
                        .rposition(|m| m.role == "tool_result" && m.tool_id.as_deref() == Some(&id))
                    {
                        self.messages[pos].content = summary;
                    } else {
                        // No progress message found — insert normally
                        let insert_pos = self.tool_insert_position(&id);
                        let mut msg = ChatMessage::new("tool_result", summary);
                        msg.tool_id = Some(id);
                        self.messages.insert(insert_pos, msg);
                    }
                } else {
                    let formatted = format_tool_result_named(&name, &content, is_err);
                    let insert_pos = self.tool_insert_position(&id);
                    let mut msg = ChatMessage::with_diff("tool_result", formatted, diff);
                    msg.tool_id = Some(id);
                    self.messages.insert(insert_pos, msg);
                }
            }
            StreamEvent::Usage {
                input_tokens,
                output_tokens,
                cache_creation_tokens,
                cache_read_tokens,
            } => {
                self.input_tokens += input_tokens;
                self.output_tokens += output_tokens;
                self.cache_read_tokens += cache_read_tokens.unwrap_or(0);
                self.cache_creation_tokens += cache_creation_tokens.unwrap_or(0);
                // The input_tokens of the latest call IS the current context size
                self.context_size = input_tokens;
            }
            StreamEvent::MessageEnd { stop_reason } => {
                // Flush any remaining thinking first (e.g. a reasoning-only
                // turn that ends without emitting text).
                if !self.streaming_thinking.is_empty() {
                    let content = std::mem::take(&mut self.streaming_thinking);
                    let trimmed = content.trim().to_string();
                    if !trimmed.is_empty() {
                        self.messages.push(ChatMessage::new("thinking", trimmed));
                    }
                }
                let content = std::mem::take(&mut self.streaming_text);
                let trimmed = content.trim_end().to_string();
                if !trimmed.is_empty() {
                    if let Some(idx) = self.turn_text_msg_idx {
                        if idx < self.messages.len() && self.messages[idx].role == "assistant" {
                            self.messages[idx].content.push_str(&trimmed);
                        } else {
                            self.messages.push(ChatMessage::new("assistant", trimmed));
                        }
                    } else {
                        self.messages.push(ChatMessage::new("assistant", trimmed));
                    }
                }
                self.turn_text_msg_idx = None;
                // When stop_reason is ToolUse, tools are about to execute —
                // keep is_streaming=true so Alt+B works and the UI shows
                // "streaming" status. ToolResult events will follow, and
                // the final MessageEnd (with EndTurn) will set it to false.
                // In daemon mode, queued messages are submitted after
                // ProcessingComplete (not here) to avoid "already processing".

                if stop_reason != StopReason::ToolUse {
                    // In daemon mode, keep is_streaming=true until ProcessingComplete
                    // so that Enter enqueues instead of sending (avoids "already processing").
                    if !self.is_daemon_mode() {
                        self.is_streaming = false;
                    }
                    self.active_tool = None;
                }
            }
            StreamEvent::ApprovalRequired { tools } => {
                let count = tools.len();
                let all_read_only = !tools.is_empty()
                    && tools.iter().all(|t| t.read_only.unwrap_or(false));
                self.approval_prompt = Some(ApprovalPrompt {
                    selections: vec![true; count],
                    selected: 0,
                    tools,
                    all_read_only,
                });
            }
            StreamEvent::PlanReview {
                id,
                title,
                plan,
                tasks,
            } => {
                self.plan_review = Some(PlanReviewState {
                    id,
                    title,
                    plan,
                    tasks,
                    selected: 0,
                    mode: PlanReviewMode::List,
                    feedback_input: String::new(),
                    scroll: 0,
                });
            }
            StreamEvent::PlannerQuestion { id, questions } => {
                let n = questions.len();
                let multi_sels: Vec<Vec<bool>> = questions
                    .iter()
                    .map(|q| vec![false; q.options.len()])
                    .collect();
                self.planner_question = Some(PlannerQuestionState {
                    id,
                    questions,
                    current_question: 0,
                    selections: vec![0; n],
                    multi_selections: multi_sels,
                    editing_custom: false,
                    custom_inputs: vec![String::new(); n],
                });
            }
            StreamEvent::ModeChanged { mode } => {
                if let Ok(parsed) = serde_json::from_value::<PermissionMode>(
                    serde_json::Value::String(mode.clone()),
                ) {
                    self.permission_mode = parsed;
                }
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Mode changed to: {mode}"),
                ));
            }
            StreamEvent::Error { error } => {
                self.messages
                    .push(ChatMessage::new("assistant", format!("Error: {error}")));
                self.is_streaming = false;
                self.queued_messages.lock().unwrap().clear();
            }
            StreamEvent::ExploreProgress { id, activity, .. } => {
                // Find existing progress message for this explore ID
                let existing = self
                    .messages
                    .iter()
                    .rposition(|m| m.role == "tool_result" && m.tool_id.as_deref() == Some(&id));

                if let Some(idx) = existing {
                    self.messages[idx].content = activity;
                } else {
                    let insert_pos = self.tool_insert_position(&id);
                    let mut msg = ChatMessage::new("tool_result", activity);
                    msg.tool_id = Some(id);
                    self.messages.insert(insert_pos, msg);
                }
            }
            StreamEvent::SystemNotification {
                source,
                level,
                detail,
            } => {
                let msg = format!("[{level}] {source}: {detail}");
                self.toast_notifications.push((msg, Instant::now()));
            }
            StreamEvent::QueuedMessageInjected { text, display_text } => {
                // Flush any partial streaming text before inserting the user message
                if !self.streaming_text.is_empty() {
                    let content = std::mem::take(&mut self.streaming_text);
                    self.messages.push(ChatMessage::new("assistant", content));
                }
                let visible_text = display_text.as_deref().unwrap_or(&text);
                self.messages.push(ChatMessage::new("user", visible_text));
                // Remove the injected message from the local queue so the UI
                // stops showing it in the "↳ queued" indicator.
                let mut queue = self.queued_messages.lock().unwrap();
                if let Some(pos) = queue.iter().position(|m| m == &text) {
                    queue.remove(pos);
                }
            }
            StreamEvent::SubAgentUpdate {
                id,
                task,
                status,
                turns,
                input_tokens,
                output_tokens,
                error,
                tab_id,
                chat_messages,
            } => {
                // Convert daemon stream event into the in-process SubAgentUpdate format
                let update = SubAgentUpdate {
                    id,
                    task,
                    status,
                    turns: turns as usize,
                    input_tokens,
                    output_tokens,
                    error,
                    tab_id,
                    chat_messages: chat_messages
                        .into_iter()
                        .map(|m| lukan_agent::sub_agent::SubAgentChatMsg {
                            role: m.role,
                            content: m.content,
                        })
                        .collect(),
                };
                self.handle_subagent_update(update);
            }
            StreamEvent::BashBackgroundCompletion {
                pid: _,
                text,
                display_text,
                tab_id,
            } => {
                self.maybe_forward_bash_completion(0, &text, display_text.as_deref(), tab_id.as_deref());
            }
            _ => {}
        }
    }
}
