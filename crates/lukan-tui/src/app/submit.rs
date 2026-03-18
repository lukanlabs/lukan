use super::helpers::build_system_prompt_with_opts;
use super::*;

impl App {
    pub(super) async fn submit_message(&mut self, agent_tx: mpsc::Sender<StreamEvent>) {
        let text = self.input.trim().to_string();
        let display = self.display_input().trim().to_string();
        self.input.clear();
        self.cursor_pos = 0;
        self.paste_info = None;

        // Handle /exit
        if text == "/exit" {
            self.should_quit = true;
            return;
        }

        // Handle /resume command
        if text == "/resume" {
            self.open_session_picker().await;
            return;
        }

        // Handle /model command
        if text == "/model" || text.starts_with("/model ") {
            self.open_model_picker().await;
            return;
        }

        // Handle /clear
        if text == "/clear" {
            self.messages.clear();
            self.committed_msg_idx = 0;
            self.viewport_scroll = 0;
            self.input_tokens = 0;
            self.output_tokens = 0;
            self.cache_read_tokens = 0;
            self.cache_creation_tokens = 0;
            self.context_size = 0;
            // Reset agent — a new session will be created on next message
            if let Some(ref daemon) = self.daemon_tx {
                let _ = daemon.send(&crate::ws_client::OutMessage::NewSession {
                    name: None,
                    session_id: self.daemon_tab_id.clone(),
                });
            }
            self.agent = None;
            self.session_id = None;
            return;
        }

        // Handle /compact
        if text == "/compact" {
            if let Some(ref daemon) = self.daemon_tx {
                let _ = daemon.send(&crate::ws_client::OutMessage::Compact {
                    session_id: self.daemon_tab_id.clone(),
                });
                self.messages
                    .push(ChatMessage::new("system", "Compacting session..."));
                return;
            }
            if self.is_streaming {
                self.messages.push(ChatMessage::new(
                    "system",
                    "Cannot compact while a response is streaming.",
                ));
                return;
            }
            let agent = match self.agent.take() {
                Some(a) => a,
                None => {
                    self.messages
                        .push(ChatMessage::new("system", "No active session to compact."));
                    return;
                }
            };
            self.is_streaming = true;
            let msg_before = agent.message_count();
            let (return_tx, return_rx) = tokio::sync::oneshot::channel::<AgentLoop>();
            self.agent_return_rx = Some(return_rx);
            let tx = agent_tx.clone();
            let mut agent = agent;
            tokio::spawn(async move {
                if let Err(e) = agent.compact(tx.clone()).await {
                    let _ = tx
                        .send(StreamEvent::Error {
                            error: e.to_string(),
                        })
                        .await;
                } else {
                    let msg_after = agent.message_count();
                    let summary = format!(
                        "Compacted: {} messages → {} messages.",
                        msg_before, msg_after
                    );
                    // TextDelta must come before MessageEnd so it gets flushed
                    // into messages when MessageEnd sets is_streaming = false.
                    let _ = tx.send(StreamEvent::TextDelta { text: summary }).await;
                    let _ = tx
                        .send(StreamEvent::MessageEnd {
                            stop_reason: lukan_core::models::events::StopReason::EndTurn,
                        })
                        .await;
                }
                let _ = return_tx.send(agent);
            });
            return;
        }

        // Handle /memories [activate | deactivate | add <text> | show]
        if text == "/memories" || text.starts_with("/memories ") {
            let sub = text
                .strip_prefix("/memories")
                .unwrap_or("")
                .trim()
                .to_string();
            let memory_dir = LukanPaths::project_memory_dir();
            let memory_path = LukanPaths::project_memory_file();
            let active_path = LukanPaths::project_memory_active_file();
            let mut did_change = false;
            if sub == "activate" {
                let _ = tokio::fs::create_dir_all(&memory_dir).await;
                if !memory_path.exists() {
                    let _ = tokio::fs::write(&memory_path, "# Project Memory\n\n").await;
                }
                let _ = tokio::fs::write(&active_path, "").await;
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Project memory activated: {}", memory_path.display()),
                ));
                did_change = true;
            } else if sub == "deactivate" {
                let _ = tokio::fs::remove_file(&active_path).await;
                self.messages.push(ChatMessage::new(
                    "system",
                    "Project memory deactivated (file preserved).",
                ));
                did_change = true;
            } else if sub == "show" {
                let content = tokio::fs::read_to_string(&memory_path)
                    .await
                    .unwrap_or_else(|_| "(empty)".to_string());
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Project Memory:\n{content}"),
                ));
            } else if sub.starts_with("add") {
                let entry = sub.strip_prefix("add").unwrap_or("").trim().to_string();
                if entry.is_empty() {
                    self.messages
                        .push(ChatMessage::new("system", "Usage: /memories add <text>"));
                } else {
                    // Auto-activate if needed
                    let _ = tokio::fs::create_dir_all(&memory_dir).await;
                    if !active_path.exists() {
                        let _ = tokio::fs::write(&active_path, "").await;
                    }
                    let current = tokio::fs::read_to_string(&memory_path)
                        .await
                        .unwrap_or_else(|_| "# Project Memory\n\n".to_string());
                    let updated = format!("{current}\n- {entry}\n");
                    let _ = tokio::fs::write(&memory_path, &updated).await;
                    self.messages.push(ChatMessage::new(
                        "system",
                        format!("Project memory updated: \"{entry}\""),
                    ));
                    did_change = true;
                }
            } else {
                let active = active_path.exists();
                self.messages.push(ChatMessage::new(
                    "system",
                    format!(
                        "Project memory: {}. Usage: /memories activate | deactivate | show | add <text>",
                        if active { "active" } else { "inactive" }
                    ),
                ));
            }
            if did_change && let Some(agent) = self.agent.as_mut() {
                agent.reload_system_prompt(build_system_prompt_with_opts(self.browser_tools).await);
            }
            return;
        }

        // Handle /gmemory [show | add <text> | clear]
        if text == "/gmemory" || text.starts_with("/gmemory ") {
            let sub = text
                .strip_prefix("/gmemory")
                .unwrap_or("")
                .trim()
                .to_string();
            let memory_path = LukanPaths::global_memory_file();
            let mut did_change = false;
            if sub == "show" {
                let content = tokio::fs::read_to_string(&memory_path)
                    .await
                    .unwrap_or_else(|_| "(empty)".to_string());
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Global Memory:\n{content}"),
                ));
            } else if sub.starts_with("add ") {
                let entry = sub.strip_prefix("add ").unwrap_or("").trim().to_string();
                if entry.is_empty() {
                    self.messages
                        .push(ChatMessage::new("system", "Usage: /gmemory add <text>"));
                } else {
                    let current = tokio::fs::read_to_string(&memory_path)
                        .await
                        .unwrap_or_else(|_| "# Global Memory\n\n".to_string());
                    let updated = format!("{current}\n- {entry}\n");
                    let _ = tokio::fs::write(&memory_path, &updated).await;
                    self.messages.push(ChatMessage::new(
                        "system",
                        format!("Global memory updated: \"{entry}\""),
                    ));
                    did_change = true;
                }
            } else if sub == "clear" {
                let _ = tokio::fs::write(&memory_path, "# Global Memory\n\n").await;
                self.messages
                    .push(ChatMessage::new("system", "Global memory cleared."));
                did_change = true;
            } else {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!(
                        "Global memory: {}\nUsage: /gmemory show | add <text> | clear",
                        memory_path.display()
                    ),
                ));
            }
            if did_change && let Some(agent) = self.agent.as_mut() {
                agent.reload_system_prompt(build_system_prompt_with_opts(self.browser_tools).await);
            }
            return;
        }

        // Handle /workers — open worker picker overlay
        if text == "/workers" {
            match WorkerManager::get_summaries().await {
                Ok(workers) => {
                    if workers.is_empty() {
                        self.messages
                            .push(ChatMessage::new("system", "No workers configured."));
                    } else {
                        let entries: Vec<WorkerEntry> = workers
                            .into_iter()
                            .map(|w| WorkerEntry {
                                id: w.definition.id,
                                name: w.definition.name,
                                enabled: w.definition.enabled,
                                schedule: w.definition.schedule,
                                last_run_status: w.definition.last_run_status,
                            })
                            .collect();
                        self.worker_picker = Some(WorkerPicker::new(entries));
                    }
                }
                Err(e) => {
                    self.messages.push(ChatMessage::new(
                        "system",
                        format!("Failed to list workers: {e}"),
                    ));
                }
            }
            return;
        }

        // Handle /skills
        if text == "/skills" {
            let cwd = std::env::current_dir().unwrap_or_default();
            let skills = lukan_tools::skills::discover_skills(&cwd).await;
            if skills.is_empty() {
                self.messages.push(ChatMessage::new(
                    "system",
                    "No skills found. Create one at .lukan/skills/<name>/SKILL.md",
                ));
            } else {
                let mut lines = vec![format!("Skills ({}):", skills.len())];
                for s in &skills {
                    lines.push(format!("  {} — {}", s.folder, s.description));
                }
                self.messages
                    .push(ChatMessage::new("system", lines.join("\n")));
            }
            return;
        }

        // Handle /events — switch to Event Agent view (Alt+L for events)
        if text == "/events" || text.starts_with("/events ") {
            let arg = text.strip_prefix("/events").unwrap_or("").trim();

            // /events clear — wipe history
            if arg == "clear" {
                let _ = std::fs::write(LukanPaths::events_history_file(), "");
                self.messages
                    .push(ChatMessage::new("system", "Event history cleared."));
                return;
            }

            // Switch to Event Agent view
            self.active_view = ActiveView::EventAgent;
            self.event_agent_has_unread = false;
            self.force_redraw = true;

            if self.event_messages.is_empty() {
                self.event_messages.push(ChatMessage::new(
                    "system",
                    "Event Agent view. System events will appear here.\nAlt+L to view events. Press Alt+E to return to main view.",
                ));
            }
            return;
        }

        // Handle /bg
        if text == "/bg" {
            if let Some(ref daemon) = self.daemon_tx {
                // In daemon mode, request bg processes from daemon
                let _ = daemon.send(&crate::ws_client::OutMessage::ListBgProcesses);
            } else {
                let processes = lukan_tools::bg_processes::get_bg_processes();
                if processes.is_empty() {
                    self.messages
                        .push(ChatMessage::new("system", "No background processes."));
                } else {
                    let entries: Vec<BgEntry> = processes.into_iter().map(BgEntry::from).collect();
                    self.bg_picker = Some(BgPicker::new(entries));
                }
            }
            return;
        }

        // Handle /checkpoints — open rewind picker
        if text == "/checkpoints" {
            if let Some(ref daemon) = self.daemon_tx {
                let _ = daemon.send(&crate::ws_client::OutMessage::ListCheckpoints {
                    session_id: self.daemon_tab_id.clone(),
                });
                return;
            }
            let checkpoints = self
                .agent
                .as_ref()
                .map(|a| a.checkpoints().to_vec())
                .unwrap_or_default();
            if checkpoints.is_empty() {
                self.messages.push(ChatMessage::new(
                    "system",
                    "No checkpoints in this session.",
                ));
            } else {
                let mut entries: Vec<RewindEntry> = checkpoints
                    .iter()
                    .map(|c| {
                        let additions: u32 = c.snapshots.iter().map(|s| s.additions).sum();
                        let deletions: u32 = c.snapshots.iter().map(|s| s.deletions).sum();
                        RewindEntry {
                            checkpoint_id: Some(c.id.clone()),
                            message: c.message.clone(),
                            files_changed: c.snapshots.len(),
                            additions,
                            deletions,
                        }
                    })
                    .collect();
                // Append "(current)" sentinel
                entries.push(RewindEntry {
                    checkpoint_id: None,
                    message: String::new(),
                    files_changed: 0,
                    additions: 0,
                    deletions: 0,
                });
                self.rewind_picker = Some(RewindPicker::new(entries));
            }
            return;
        }

        // Handle !command — execute shell command and add output to context
        if let Some(shell_cmd) = text.strip_prefix('!') {
            let cmd = shell_cmd.trim();
            if cmd.is_empty() {
                return;
            }
            self.messages
                .push(ChatMessage::new("system", format!("$ {cmd}")));

            let cwd = std::env::current_dir().unwrap_or_default();
            let result = tokio::process::Command::new("bash")
                .arg("-c")
                .arg(cmd)
                .current_dir(&cwd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
                .await;

            match result {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let combined = format!("{}{}", stdout, stderr).trim().to_string();
                    let exit_code = output.status.code().unwrap_or(-1);

                    // Truncate if too large
                    let truncated = if combined.len() > 30000 {
                        let start_end = combined.floor_char_boundary(15000);
                        let tail_start = combined.floor_char_boundary(combined.len() - 15000);
                        format!(
                            "{}\n\n... (truncated) ...\n\n{}",
                            &combined[..start_end],
                            &combined[tail_start..]
                        )
                    } else {
                        combined
                    };

                    let context_msg = if exit_code != 0 {
                        format!("$ {cmd}\n{truncated}\n[exit code: {exit_code}]")
                    } else {
                        format!("$ {cmd}\n{truncated}")
                    };

                    // Show output in chat
                    let display_output = if truncated.is_empty() {
                        format!("(exit code: {exit_code})")
                    } else if exit_code != 0 {
                        format!("{truncated}\n[exit code: {exit_code}]")
                    } else {
                        truncated.clone()
                    };
                    self.messages
                        .push(ChatMessage::new("system", display_output));

                    // Add to agent context
                    let agent = match self.agent.take() {
                        Some(a) => a,
                        None => self.create_agent().await,
                    };
                    let mut agent = agent;
                    agent.add_user_context(&context_msg);
                    self.session_id = Some(agent.session_id().to_string());
                    self.agent = Some(agent);
                }
                Err(e) => {
                    self.messages.push(ChatMessage::new(
                        "system",
                        format!("Failed to execute command: {e}"),
                    ));
                }
            }
            return;
        }

        // Guard: no model selected → block send, prompt user
        if self.config.effective_model().is_none() {
            self.messages.push(ChatMessage::new(
                "system",
                "No model selected. Use /model to choose one.",
            ));
            self.input = text;
            self.cursor_pos = self.input.len();
            return;
        }

        // Regular message — show truncated preview in chat, send full text to agent
        self.messages.push(ChatMessage::new("user", display));

        self.is_streaming = true;
        self.streaming_text.clear();
        self.streaming_thinking.clear();
        self.active_tool = None;

        // ── Daemon mode: send via WebSocket ──
        if let Some(ref daemon) = self.daemon_tx {
            let msg = crate::ws_client::OutMessage::SendMessage {
                content: text,
                session_id: self.daemon_tab_id.clone(),
            };
            if let Err(e) = daemon.send(&msg) {
                self.is_streaming = false;
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Failed to send to daemon: {e}"),
                ));
            }
            return;
        }

        // ── In-process mode: run agent turn directly ──
        // Ensure agent exists (create new session if needed) and run the turn
        // We need to take the agent out to avoid borrow issues with self
        if let Some(ref mut agent) = self.agent {
            agent.set_disabled_tools(self.disabled_tools.clone());
        }
        let agent = match self.agent.take() {
            Some(a) => a,
            None => self.create_agent().await,
        };

        self.session_id = Some(agent.session_id().to_string());

        // Cancellation token for ESC-to-cancel
        let cancel_token = CancellationToken::new();
        self.cancel_token = Some(cancel_token.clone());

        // Oneshot channel to get the agent back after the turn
        let (return_tx, return_rx) = tokio::sync::oneshot::channel::<AgentLoop>();
        self.agent_return_rx = Some(return_rx);

        let mut agent = agent;
        let queued = self.queued_messages.clone();
        tokio::spawn(async move {
            if let Err(e) = agent
                .run_turn(&text, agent_tx.clone(), Some(cancel_token), Some(queued))
                .await
            {
                error!("Agent loop error: {e}");
                agent_tx
                    .send(StreamEvent::Error {
                        error: e.to_string(),
                    })
                    .await
                    .ok();
            }

            // Return the agent so history persists
            let _ = return_tx.send(agent);
        });
    }
}
