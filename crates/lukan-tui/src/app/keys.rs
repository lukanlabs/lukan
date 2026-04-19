use super::helpers::filtered_commands;
use super::*;

impl App {
    pub(super) async fn handle_key_event(
        &mut self,
        key: crossterm::event::KeyEvent,
        agent_tx: &mpsc::Sender<StreamEvent>,
        event_agent_tx: &mpsc::Sender<StreamEvent>,
    ) -> bool {
        // Ctrl+Shift+F9: close and kill the terminal
        if key.code == KeyCode::F(9)
            && key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL)
            && key
                .modifiers
                .contains(crossterm::event::KeyModifiers::SHIFT)
        {
            if let Some(modal) = self.terminal_modal.take() {
                modal.close();
            }
            self.terminal_visible = false;
            self.sync_mouse_capture(false);
            self.terminal_overlay_inner = None;
            self.force_redraw = true;
            return true;
        }

        // F9: minimize/restore terminal (or open new one)
        // If the shell has exited, F9 closes it completely.
        if key.code == KeyCode::F(9) {
            if let Some(ref modal) = self.terminal_modal {
                if modal.has_exited() {
                    // Shell exited — close completely
                    if let Some(modal) = self.terminal_modal.take() {
                        modal.close();
                    }
                    self.terminal_visible = false;
                    self.sync_mouse_capture(false);
                    self.terminal_overlay_inner = None;
                } else {
                    // Toggle visibility (minimize / restore)
                    self.terminal_visible = !self.terminal_visible;
                    self.sync_mouse_capture(self.terminal_visible);
                }
            } else {
                // No terminal yet — open with approximate inner size
                // (will be corrected on next frame via resize sync)
                let term_size = crossterm::terminal::size().unwrap_or((80, 24));
                let approx_chat_h = term_size.1.saturating_sub(5);
                let overlay_w = (term_size.0 * 90 / 100).max(20);
                let overlay_h = (approx_chat_h * 85 / 100).max(10);
                let inner_cols = overlay_w.saturating_sub(2);
                let inner_rows = overlay_h.saturating_sub(2);
                match TerminalModal::open(inner_cols, inner_rows) {
                    Ok(modal) => {
                        self.terminal_modal = Some(modal);
                        self.terminal_visible = true;
                        self.sync_mouse_capture(true);
                    }
                    Err(e) => {
                        self.toast_notifications
                            .push((format!("Failed to open terminal: {e}"), Instant::now()));
                    }
                }
            }
            self.force_redraw = true;
            return true;
        }

        // When terminal modal is visible, handle scroll or forward to PTY
        if self.terminal_visible
            && let Some(ref mut modal) = self.terminal_modal
        {
            let ctrl = key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL);
            match key.code {
                // Ctrl+Up: scroll up 1 line
                KeyCode::Up if ctrl => {
                    modal.screen_mut().scroll_up(1);
                }
                // Ctrl+Down: scroll down 1 line
                KeyCode::Down if ctrl => {
                    modal.screen_mut().scroll_down(1);
                }
                // Ctrl+PageUp: scroll up half page
                KeyCode::PageUp if ctrl => {
                    let half = (modal.screen().size().0 / 2).max(1) as usize;
                    modal.screen_mut().scroll_up(half);
                }
                // Ctrl+PageDown: scroll down half page
                KeyCode::PageDown if ctrl => {
                    let half = (modal.screen().size().0 / 2).max(1) as usize;
                    modal.screen_mut().scroll_down(half);
                }
                _ => {
                    // Any other key snaps to live view and forwards to PTY
                    if modal.screen().scroll_offset > 0 {
                        modal.screen_mut().snap_to_bottom();
                    }
                    // Clear selection on any keypress
                    modal.clear_selection();
                    modal.send_key(&key);
                }
            }
            // Drain PTY output immediately so screen stays in sync
            // (without this, rapid key repeats starve the Tick handler)
            modal.process_output();
            return true;
        }

        if self.trust_prompt.is_some() {
            // Trust prompt overlay
            match key.code {
                KeyCode::Up => {
                    self.trust_prompt.as_mut().unwrap().selected = 0;
                }
                KeyCode::Down => {
                    self.trust_prompt.as_mut().unwrap().selected = 1;
                }
                KeyCode::Enter => {
                    let selected = self.trust_prompt.as_ref().unwrap().selected;
                    if selected == 0 {
                        // Trust — persist and continue
                        let cwd = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());
                        let _ = lukan_core::config::ProjectConfig::mark_trusted(&cwd).await;
                        self.trust_prompt = None;
                        self.force_redraw = true;
                    } else {
                        // No trust — exit
                        self.should_quit = true;
                    }
                }
                KeyCode::Esc => {
                    self.should_quit = true;
                }
                _ => {}
            }
        } else if self.plan_review.is_some() {
            // Plan review overlay
            self.handle_plan_review_key(key.code);
        } else if self.planner_question.is_some() {
            // Planner question overlay
            self.handle_planner_question_key(key.code);
        } else if self.approval_prompt.is_some() {
            // Approval prompt overlay
            match key.code {
                KeyCode::Up => {
                    if let Some(ref mut prompt) = self.approval_prompt
                        && prompt.selected > 0
                    {
                        prompt.selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut prompt) = self.approval_prompt
                        && prompt.selected + 1 < prompt.tools.len()
                    {
                        prompt.selected += 1;
                    }
                }
                KeyCode::Char(' ') => {
                    // Toggle individual tool approval
                    if let Some(ref mut prompt) = self.approval_prompt {
                        let idx = prompt.selected;
                        if idx < prompt.selections.len() {
                            prompt.selections[idx] = !prompt.selections[idx];
                        }
                    }
                }
                KeyCode::Enter => {
                    // Submit selections
                    if let Some(prompt) = self.approval_prompt.take() {
                        let approved_ids: Vec<String> = prompt
                            .tools
                            .iter()
                            .zip(prompt.selections.iter())
                            .filter(|(_, sel)| **sel)
                            .map(|(t, _)| t.id.clone())
                            .collect();
                        let response = if approved_ids.is_empty() {
                            ApprovalResponse::DeniedAll
                        } else {
                            ApprovalResponse::Approved { approved_ids }
                        };
                        self.send_approval(response);
                        self.force_redraw = true;
                    }
                }
                KeyCode::Char('a') => {
                    // Approve all and submit
                    if let Some(prompt) = self.approval_prompt.take() {
                        let approved_ids: Vec<String> =
                            prompt.tools.iter().map(|t| t.id.clone()).collect();
                        self.send_approval(ApprovalResponse::Approved { approved_ids });
                        self.force_redraw = true;
                    }
                }
                KeyCode::Char('A') => {
                    // Always allow — approve all + persist patterns to config
                    if let Some(prompt) = self.approval_prompt.take() {
                        let approved_ids: Vec<String> =
                            prompt.tools.iter().map(|t| t.id.clone()).collect();
                        let tools = prompt.tools.clone();
                        self.send_approval(ApprovalResponse::AlwaysAllow {
                            approved_ids,
                            tools,
                        });
                        self.force_redraw = true;
                    }
                }
                KeyCode::Esc => {
                    // Deny all
                    self.approval_prompt = None;
                    self.send_approval(ApprovalResponse::DeniedAll);
                    self.force_redraw = true;
                }
                _ => {}
            }
        } else if self.tool_picker.is_some() {
            match key.code {
                KeyCode::Up => {
                    if let Some(ref mut picker) = self.tool_picker
                        && picker.selected > 0
                    {
                        picker.selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut picker) = self.tool_picker {
                        let total = Self::tool_picker_tool_count(picker);
                        if picker.selected + 1 < total {
                            picker.selected += 1;
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    if let Some(ref mut picker) = self.tool_picker
                        && let Some(tool_name) = Self::tool_picker_selected_tool_name(picker)
                    {
                        if picker.disabled.contains(&tool_name) {
                            picker.disabled.remove(&tool_name);
                        } else {
                            picker.disabled.insert(tool_name);
                        }
                    }
                }
                KeyCode::Esc => {
                    self.close_tool_picker();
                }
                KeyCode::Char('p')
                    if key.modifiers.contains(crossterm::event::KeyModifiers::ALT) =>
                {
                    self.close_tool_picker();
                }
                KeyCode::Enter => {
                    // Block Enter while tool picker is open
                }
                _ => {}
            }
        } else if self.event_picker.is_some() {
            // Unified event picker / log key handling
            let mode = self.event_picker.as_ref().unwrap().mode;
            match key.code {
                KeyCode::Left => {
                    if let Some(ref mut picker) = self.event_picker {
                        picker.prev_tab();
                    }
                }
                KeyCode::Right => {
                    if let Some(ref mut picker) = self.event_picker {
                        picker.next_tab();
                    }
                }
                KeyCode::Up => {
                    if let Some(ref mut picker) = self.event_picker {
                        match mode {
                            EventPickerMode::Picker => {
                                if picker.cursor > 0 {
                                    picker.cursor -= 1;
                                }
                            }
                            EventPickerMode::Log => {
                                picker.log_scroll = picker.log_scroll.saturating_sub(1);
                            }
                        }
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut picker) = self.event_picker {
                        match mode {
                            EventPickerMode::Picker => {
                                let max = picker.filtered_entry_indices().len().saturating_sub(1);
                                if picker.cursor < max {
                                    picker.cursor += 1;
                                }
                            }
                            EventPickerMode::Log => {
                                picker.log_scroll = picker.log_scroll.saturating_add(1);
                            }
                        }
                    }
                }
                KeyCode::Char(' ') if mode == EventPickerMode::Picker => {
                    if let Some(ref mut picker) = self.event_picker {
                        picker.toggle_current();
                    }
                }
                KeyCode::Char('a') if mode == EventPickerMode::Picker => {
                    if let Some(ref mut picker) = self.event_picker {
                        picker.select_all();
                    }
                }
                KeyCode::Char('n') if mode == EventPickerMode::Picker => {
                    if let Some(ref mut picker) = self.event_picker {
                        picker.deselect_all();
                    }
                }
                KeyCode::Enter if mode == EventPickerMode::Picker => {
                    // Send selected events to event agent and trigger review
                    let mut has_events = false;
                    if let Some(ref mut picker) = self.event_picker {
                        let (selected, remaining) = picker.take_selected();
                        self.event_buffer.extend(remaining);
                        if !selected.is_empty() {
                            has_events = true;
                            self.event_buffer.extend(selected);
                        }
                    }
                    self.event_picker = None;
                    self.force_redraw = true;
                    if has_events {
                        self.trigger_event_agent_auto_turn(event_agent_tx.clone())
                            .await;
                    }
                }
                KeyCode::Esc => {
                    // Close — return events to buffer in picker mode
                    if let Some(ref mut picker) = self.event_picker
                        && picker.mode == EventPickerMode::Picker
                    {
                        let returned = picker.return_all();
                        self.event_buffer.extend(returned);
                    }
                    self.event_picker = None;
                    self.force_redraw = true;
                }
                _ => {}
            }
        } else if self.memory_viewer.is_some() {
            // Memory viewer overlay — scroll with arrow keys, PgUp/PgDown, Home/End, ESC closes
            let scroll_step: u16 = 3;
            let page_step: u16 = 15;
            match key.code {
                KeyCode::Esc => {
                    self.memory_viewer = None;
                    self.memory_viewer_scroll = 0;
                    self.force_redraw = true;
                }
                KeyCode::Up => {
                    self.memory_viewer_scroll =
                        self.memory_viewer_scroll.saturating_sub(scroll_step);
                    self.force_redraw = true;
                }
                KeyCode::Down => {
                    self.memory_viewer_scroll =
                        self.memory_viewer_scroll.saturating_add(scroll_step);
                    self.force_redraw = true;
                }
                KeyCode::PageUp => {
                    self.memory_viewer_scroll = self.memory_viewer_scroll.saturating_sub(page_step);
                    self.force_redraw = true;
                }
                KeyCode::PageDown => {
                    self.memory_viewer_scroll = self.memory_viewer_scroll.saturating_add(page_step);
                    self.force_redraw = true;
                }
                KeyCode::Home => {
                    self.memory_viewer_scroll = 0;
                    self.force_redraw = true;
                }
                _ => {}
            }
        } else if self.rewind_picker.is_some() {
            // Rewind picker overlay
            match key.code {
                KeyCode::Up => {
                    if let Some(ref mut picker) = self.rewind_picker {
                        if picker.view == RewindView::List && picker.selected > 0 {
                            picker.selected -= 1;
                        } else if picker.view == RewindView::Options && picker.option_idx > 0 {
                            picker.option_idx -= 1;
                        }
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut picker) = self.rewind_picker {
                        if picker.view == RewindView::List
                            && picker.selected + 1 < picker.entries.len()
                        {
                            picker.selected += 1;
                        } else if picker.view == RewindView::Options && picker.option_idx < 1 {
                            picker.option_idx += 1;
                        }
                    }
                }
                KeyCode::Enter => {
                    if let Some(ref mut picker) = self.rewind_picker {
                        if picker.view == RewindView::List {
                            // Can't restore "(current)" — it has no checkpoint_id
                            if picker.selected_checkpoint_id().is_some() {
                                picker.view = RewindView::Options;
                                picker.option_idx = 0;
                            }
                        } else {
                            // Options view — perform the restore
                            let restore_code = picker.option_idx == 1;
                            let checkpoint_id =
                                picker.selected_checkpoint_id().map(|s| s.to_string());

                            if let Some(id) = checkpoint_id {
                                self.restore_to_checkpoint(&id, restore_code).await;
                            }
                            self.rewind_picker = None;
                            self.force_redraw = true;
                        }
                    }
                }
                KeyCode::Esc => {
                    if let Some(ref mut picker) = self.rewind_picker {
                        if picker.view == RewindView::Options {
                            picker.view = RewindView::List;
                        } else {
                            self.rewind_picker = None;
                            self.force_redraw = true;
                        }
                    }
                }
                _ => {}
            }
        } else if self.bg_picker.is_some() {
            // Background process picker mode
            match key.code {
                KeyCode::Up => {
                    if let Some(ref mut picker) = self.bg_picker {
                        match picker.view {
                            BgPickerView::List => {
                                if picker.selected > 0 {
                                    picker.selected -= 1;
                                }
                            }
                            BgPickerView::Log => {
                                picker.log_scroll = picker.log_scroll.saturating_sub(3);
                            }
                        }
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut picker) = self.bg_picker {
                        match picker.view {
                            BgPickerView::List => {
                                if picker.selected + 1 < picker.entries.len() {
                                    picker.selected += 1;
                                }
                            }
                            BgPickerView::Log => {
                                picker.log_scroll = picker.log_scroll.saturating_add(3);
                            }
                        }
                    }
                }
                KeyCode::PageUp => {
                    if let Some(ref mut picker) = self.bg_picker
                        && picker.view == BgPickerView::Log
                    {
                        picker.log_scroll = picker.log_scroll.saturating_sub(15);
                    }
                }
                KeyCode::PageDown => {
                    if let Some(ref mut picker) = self.bg_picker
                        && picker.view == BgPickerView::Log
                    {
                        picker.log_scroll = picker.log_scroll.saturating_add(15);
                    }
                }
                KeyCode::Home => {
                    if let Some(ref mut picker) = self.bg_picker
                        && picker.view == BgPickerView::Log
                    {
                        picker.log_scroll = 0;
                    }
                }
                KeyCode::End => {
                    if let Some(ref mut picker) = self.bg_picker
                        && picker.view == BgPickerView::Log
                    {
                        // Jump to end: set scroll to a large value, render will clamp
                        picker.log_scroll = u16::MAX;
                    }
                }
                KeyCode::Char('l') | KeyCode::Enter => {
                    if let Some(ref mut picker) = self.bg_picker
                        && picker.view == BgPickerView::List
                    {
                        if let Some(ref daemon) = self.daemon_tx {
                            // Daemon mode: request log from daemon
                            if let Some(pid) = picker.selected_pid() {
                                picker.log_pid = pid;
                                picker.view = BgPickerView::Log;
                                let _ = daemon
                                    .send(&crate::ws_client::OutMessage::GetBgProcessLog { pid });
                            }
                        } else {
                            picker.load_log();
                        }
                    }
                }
                KeyCode::Char('k') | KeyCode::Delete => {
                    if let Some(ref mut picker) = self.bg_picker {
                        let pid = if picker.view == BgPickerView::Log {
                            Some(picker.log_pid)
                        } else {
                            picker.selected_pid()
                        };
                        if let Some(pid) = pid {
                            // Get command name before killing for the message
                            let cmd_preview = picker
                                .entries
                                .iter()
                                .find(|e| e.pid == pid)
                                .map(|e| {
                                    if e.command.len() > 40 {
                                        let end = e.command.floor_char_boundary(39);
                                        format!("{}…", &e.command[..end])
                                    } else {
                                        e.command.clone()
                                    }
                                })
                                .unwrap_or_default();

                            let was_alive = lukan_tools::bg_processes::is_process_alive(pid);
                            if was_alive {
                                lukan_tools::bg_processes::kill_bg_process(pid);
                            }
                            // Remove from tracker so it disappears from the list
                            lukan_tools::bg_processes::remove_bg_process(pid);

                            // Show confirmation
                            let action = if was_alive { "Killed" } else { "Removed" };
                            self.messages.push(ChatMessage::new(
                                "system",
                                format!("{action} process {pid} ({cmd_preview})"),
                            ));

                            // Refresh and go back to list view
                            picker.refresh();
                            if picker.view == BgPickerView::Log {
                                picker.view = BgPickerView::List;
                            }

                            // Close picker if no more processes
                            if picker.entries.is_empty() {
                                self.bg_picker = None;
                                self.force_redraw = true;
                            }
                        }
                    }
                }
                KeyCode::Esc => {
                    if let Some(ref mut picker) = self.bg_picker {
                        if picker.view == BgPickerView::Log {
                            picker.view = BgPickerView::List;
                            picker.log_scroll = 0;
                        } else {
                            self.bg_picker = None;
                            self.force_redraw = true;
                        }
                    }
                }
                _ => {}
            }
        } else if self.worker_picker.is_some() {
            // Worker picker mode
            match key.code {
                KeyCode::Up => {
                    if let Some(ref mut picker) = self.worker_picker {
                        match picker.view {
                            WorkerPickerView::WorkerList => {
                                if picker.selected > 0 {
                                    picker.selected -= 1;
                                }
                            }
                            WorkerPickerView::RunList => {
                                if picker.run_selected > 0 {
                                    picker.run_selected -= 1;
                                }
                            }
                            WorkerPickerView::RunDetail => {}
                        }
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut picker) = self.worker_picker {
                        match picker.view {
                            WorkerPickerView::WorkerList => {
                                if picker.selected + 1 < picker.entries.len() {
                                    picker.selected += 1;
                                }
                            }
                            WorkerPickerView::RunList => {
                                if picker.run_selected + 1 < picker.runs.len() {
                                    picker.run_selected += 1;
                                }
                            }
                            WorkerPickerView::RunDetail => {}
                        }
                    }
                }
                KeyCode::Enter => {
                    if let Some(ref mut picker) = self.worker_picker {
                        match picker.view {
                            WorkerPickerView::WorkerList => {
                                // Load runs for selected worker
                                if let Some(entry) = picker.selected_worker() {
                                    let worker_id = entry.id.clone();
                                    let worker_name = entry.name.clone();
                                    match WorkerManager::get_detail(&worker_id).await {
                                        Ok(Some(detail)) => {
                                            picker.runs = detail
                                                .recent_runs
                                                .into_iter()
                                                .map(|r| RunEntry {
                                                    id: r.id,
                                                    status: r.status,
                                                    started_at: r.started_at,
                                                    turns: r.turns,
                                                })
                                                .collect();
                                            picker.run_selected = 0;
                                            picker.selected_worker_name = worker_name;
                                            picker.selected_worker_id = worker_id;
                                            picker.view = WorkerPickerView::RunList;
                                        }
                                        Ok(None) => {
                                            picker.runs = Vec::new();
                                            picker.run_selected = 0;
                                            picker.selected_worker_name = worker_name;
                                            picker.selected_worker_id = worker_id;
                                            picker.view = WorkerPickerView::RunList;
                                        }
                                        Err(_) => {}
                                    }
                                }
                            }
                            WorkerPickerView::RunList => {
                                // Load run detail
                                if let Some(run) = picker.selected_run() {
                                    let run_id = run.id.clone();
                                    let run_status = run.status.clone();
                                    let worker_id = picker.selected_worker_id.clone();
                                    match WorkerManager::get_run(&worker_id, &run_id).await {
                                        Ok(Some(full_run)) => {
                                            picker.run_output = full_run.output;
                                            picker.run_status = run_status;
                                            picker.run_id = run_id;
                                            picker.view = WorkerPickerView::RunDetail;
                                        }
                                        Ok(None) => {
                                            picker.run_output = "(run not found)".to_string();
                                            picker.run_status = run_status;
                                            picker.run_id = run_id;
                                            picker.view = WorkerPickerView::RunDetail;
                                        }
                                        Err(_) => {}
                                    }
                                }
                            }
                            WorkerPickerView::RunDetail => {}
                        }
                    }
                }
                KeyCode::Esc => {
                    if let Some(ref mut picker) = self.worker_picker {
                        match picker.view {
                            WorkerPickerView::RunDetail => {
                                picker.view = WorkerPickerView::RunList;
                            }
                            WorkerPickerView::RunList => {
                                picker.view = WorkerPickerView::WorkerList;
                            }
                            WorkerPickerView::WorkerList => {
                                self.worker_picker = None;
                                self.force_redraw = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        } else if self.subagent_picker.is_some() {
            // SubAgent picker mode
            match key.code {
                KeyCode::Up => {
                    if let Some(ref mut picker) = self.subagent_picker {
                        match picker.view {
                            SubAgentPickerView::List => {
                                if picker.selected > 0 {
                                    picker.selected -= 1;
                                }
                            }
                            SubAgentPickerView::ChatDetail => {
                                // Up = scroll back (increase offset from bottom)
                                picker.scroll_offset = picker.scroll_offset.saturating_add(3);
                            }
                        }
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut picker) = self.subagent_picker {
                        match picker.view {
                            SubAgentPickerView::List => {
                                if picker.selected + 1 < picker.entries.len() {
                                    picker.selected += 1;
                                }
                            }
                            SubAgentPickerView::ChatDetail => {
                                // Down = scroll forward (decrease offset toward bottom)
                                picker.scroll_offset = picker.scroll_offset.saturating_sub(3);
                            }
                        }
                    }
                }
                KeyCode::Enter => {
                    if let Some(ref mut picker) = self.subagent_picker
                        && picker.view == SubAgentPickerView::List
                        && let Some(entry) = picker.selected_entry()
                    {
                        let entry_id = entry.id.clone();
                        // Fetch fresh data
                        let agents = get_all_sub_agents().await;
                        if let Some(agent) = agents.iter().find(|a| a.id == entry_id) {
                            picker.detail_id = agent.id.clone();
                            picker.detail_status = format!("{}", agent.status);
                            picker.detail_turns = format!("{}", agent.turns);
                            picker.detail_tokens = format!(
                                "{}in/{}out tokens",
                                agent.input_tokens, agent.output_tokens
                            );
                            picker.detail_error = agent.error.clone();
                            // Convert SubAgentChatMsg to ChatMessage for rendering
                            picker.detail_messages = agent
                                .chat_messages
                                .iter()
                                .map(|m| ChatMessage::new(&m.role, &m.content))
                                .collect();
                            picker.scroll_offset = 0;
                            picker.view = SubAgentPickerView::ChatDetail;
                        }
                    }
                }
                KeyCode::Esc => {
                    if let Some(ref mut picker) = self.subagent_picker {
                        match picker.view {
                            SubAgentPickerView::ChatDetail => {
                                picker.view = SubAgentPickerView::List;
                            }
                            SubAgentPickerView::List => {
                                self.subagent_picker = None;
                                self.force_redraw = true;
                            }
                        }
                    }
                }
                KeyCode::Char('k') => {
                    // Kill selected subagent
                    if let Some(ref picker) = self.subagent_picker
                        && picker.view == SubAgentPickerView::List
                        && let Some(entry) = picker.selected_entry()
                    {
                        let entry_id = entry.id.clone();
                        // In daemon mode, send abort via WS; in-process mode, abort locally.
                        // Both paths also mark the local manager entry as Aborted so the
                        // picker refresh below sees the updated status immediately.
                        if let Some(ref tx) = self.daemon_tx {
                            let msg = crate::ws_client::OutMessage::AbortSubAgent {
                                id: entry_id.clone(),
                            };
                            let _ = tx.send(&msg);
                        }
                        // Always call local abort — in daemon mode this marks
                        // the local mirror entry as Aborted; in in-process mode
                        // it cancels the actual token.
                        lukan_agent::sub_agent::abort_sub_agent(&entry_id).await;

                        self.messages.push(ChatMessage::new(
                            "system",
                            format!("SubAgent {entry_id} killed"),
                        ));
                        // Refresh picker — aborted entries are excluded
                        let agents = get_all_sub_agents().await;
                        let running: Vec<_> = agents
                            .iter()
                            .filter(|a| a.status == lukan_agent::sub_agent::SubAgentStatus::Running)
                            .collect();
                        if running.is_empty() {
                            self.subagent_picker = None;
                            self.force_redraw = true;
                        } else {
                            let entries: Vec<SubAgentDisplayEntry> = running
                                .iter()
                                .map(|a| {
                                    let secs = chrono::Utc::now()
                                        .signed_duration_since(a.started_at)
                                        .num_seconds();
                                    let task_preview = if a.task.len() > 60 {
                                        let end = a.task.floor_char_boundary(57);
                                        format!("{}...", &a.task[..end])
                                    } else {
                                        a.task.clone()
                                    };
                                    SubAgentDisplayEntry {
                                        id: a.id.clone(),
                                        task: task_preview,
                                        status: format!("{}", a.status),
                                        turns: format!("{}", a.turns),
                                        elapsed: format!("{secs}s running"),
                                    }
                                })
                                .collect();
                            self.subagent_picker = Some(SubAgentPicker::new(entries));
                        }
                    }
                }
                _ => {}
            }
        } else if self.reasoning_picker.is_some() {
            // Reasoning effort picker mode
            match key.code {
                KeyCode::Up => {
                    if let Some(ref mut picker) = self.reasoning_picker
                        && picker.selected > 0
                    {
                        picker.selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut picker) = self.reasoning_picker
                        && picker.selected + 1 < picker.levels.len()
                    {
                        picker.selected += 1;
                    }
                }
                KeyCode::Enter => {
                    let picker = self.reasoning_picker.take().unwrap();
                    let (effort, _, _) = picker.levels[picker.selected];
                    self.apply_model_switch_with_effort(&picker.model_entry, Some(effort))
                        .await;
                    self.force_redraw = true;
                }
                KeyCode::Esc => {
                    self.reasoning_picker = None;
                    self.force_redraw = true;
                }
                _ => {}
            }
        } else if self.session_picker.is_some() {
            // Session picker mode
            match key.code {
                KeyCode::Up => {
                    if let Some(ref mut picker) = self.session_picker
                        && picker.selected > 0
                    {
                        picker.selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut picker) = self.session_picker
                        && picker.selected + 1 < picker.sessions.len()
                    {
                        picker.selected += 1;
                    }
                }
                KeyCode::Enter => {
                    let idx = self.session_picker.as_ref().unwrap().selected;
                    self.load_selected_session(idx).await;
                    self.session_picker = None;
                    self.force_redraw = true;
                }
                KeyCode::Esc => {
                    self.session_picker = None;
                    self.force_redraw = true;
                }
                _ => {}
            }
        } else if self.model_picker.is_some() {
            // Model picker mode
            match key.code {
                KeyCode::Up => {
                    if let Some(ref mut picker) = self.model_picker
                        && picker.selected > 0
                    {
                        picker.selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if let Some(ref mut picker) = self.model_picker
                        && picker.selected + 1 < picker.models.len()
                    {
                        picker.selected += 1;
                    }
                }
                KeyCode::Enter => {
                    let idx = self.model_picker.as_ref().unwrap().selected;
                    self.select_model(idx).await;
                    self.model_picker = None;
                    self.force_redraw = true;
                }
                KeyCode::Char('d') => {
                    // Set selected model as default and switch to it
                    let idx = self.model_picker.as_ref().unwrap().selected;
                    self.set_default_model(idx).await;
                    self.model_picker = None;
                    self.force_redraw = true;
                }
                KeyCode::Esc => {
                    self.model_picker = None;
                    self.force_redraw = true;
                }
                _ => {}
            }
        } else if is_quit(&key) {
            self.should_quit = true;
        } else if key.code == KeyCode::Esc && self.is_streaming {
            // ESC during streaming: clear queued messages first,
            // second ESC cancels the agent turn
            if !self.queued_messages.lock().unwrap().is_empty() {
                self.queued_messages.lock().unwrap().clear();
            } else {
                if let Some(ref daemon) = self.daemon_tx {
                    let _ = daemon.send(&crate::ws_client::OutMessage::Abort {
                        session_id: self.daemon_tab_id.clone(),
                    });
                } else if let Some(token) = self.cancel_token.take() {
                    token.cancel();
                }
                self.is_streaming = false;
                self.active_tool = None;
                // Flush any partial streaming text
                if !self.streaming_text.is_empty() {
                    let content = std::mem::take(&mut self.streaming_text);
                    self.messages.push(ChatMessage::new("assistant", content));
                }
                self.messages
                    .push(ChatMessage::new("system", "Response cancelled."));
            }
        } else if key.code == KeyCode::Enter && self.is_streaming {
            // Enter during streaming: queue the message for mid-turn injection
            if !self.input.trim().is_empty() {
                let text = self.input.trim().to_string();
                // In daemon mode, send QueueMessage so the daemon injects mid-turn
                if let Some(ref daemon) = self.daemon_tx {
                    let _ = daemon.send(&crate::ws_client::OutMessage::QueueMessage {
                        content: text.clone(),
                        display_content: None,
                        session_id: self.daemon_tab_id.clone(),
                    });
                }
                // Also queue locally (for in-process mode and for UI display)
                self.queued_messages.lock().unwrap().push(text);
                self.input.clear();
                self.cursor_pos = 0;
                self.paste_info = None;
            }
        } else if key.code == KeyCode::Up && self.is_streaming {
            // Up during streaming: dequeue messages back into input (each on its own line)
            let drained: Vec<String> = self.queued_messages.lock().unwrap().drain(..).collect();
            if !drained.is_empty() {
                let existing = self.input.trim().to_string();
                if existing.is_empty() {
                    self.input = drained.join("\n");
                } else {
                    self.input = format!("{}\n{}", drained.join("\n"), existing);
                }
                self.cursor_pos = self.input.len();
            }
        } else if key.code == KeyCode::Char('b')
            && key.modifiers.contains(crossterm::event::KeyModifiers::ALT)
            && self.is_streaming
        {
            // Alt+B: send running Bash command to background
            if let Some(ref daemon) = self.daemon_tx {
                let _ = daemon.send(&crate::ws_client::OutMessage::SendToBackground {
                    session_id: self.daemon_tab_id.clone(),
                });
            } else {
                let _ = self.bg_signal_tx.send(());
            }
            self.messages.push(ChatMessage::new(
                "system",
                "Sending current command to background...",
            ));
        } else if key.code == KeyCode::Char('p')
            && key.modifiers.contains(crossterm::event::KeyModifiers::ALT)
            && !self.is_streaming
            && !self.event_is_streaming
        {
            // Alt+P: toggle tool picker overlay
            if self.tool_picker.is_some() {
                self.close_tool_picker();
            } else {
                self.open_tool_picker();
            }
        } else if key.code == KeyCode::Char('m')
            && key.modifiers.contains(crossterm::event::KeyModifiers::ALT)
        {
            // Alt+M: show memory viewer
            let mut content = String::new();
            let global_path = LukanPaths::global_memory_file();
            if let Ok(mem) = tokio::fs::read_to_string(&global_path).await {
                let trimmed = mem.trim();
                if !trimmed.is_empty() {
                    content.push_str("── Global Memory ──\n\n");
                    content.push_str(trimmed);
                }
            }
            let active_path = LukanPaths::project_memory_active_file();
            if tokio::fs::metadata(&active_path).await.is_ok() {
                let project_path = LukanPaths::project_memory_file();
                if let Ok(mem) = tokio::fs::read_to_string(&project_path).await {
                    let trimmed = mem.trim();
                    if !trimmed.is_empty() {
                        if !content.is_empty() {
                            content.push_str("\n\n");
                        }
                        content.push_str("── Project Memory ──\n\n");
                        content.push_str(trimmed);
                    }
                }
            }
            if content.is_empty() {
                content =
                    "No memory files found.\n\nUse /memories activate to enable project memory."
                        .to_string();
            }
            self.memory_viewer = Some(content);
            self.memory_viewer_scroll = 0;
            self.force_redraw = true;
        } else if key.code == KeyCode::Char('t')
            && key.modifiers.contains(crossterm::event::KeyModifiers::ALT)
        {
            // Alt+T: toggle task panel
            self.task_panel_visible = !self.task_panel_visible;
            if self.task_panel_visible {
                let cwd = std::env::current_dir().unwrap_or_default();
                self.task_panel_entries = lukan_tools::tasks::read_all_tasks(&cwd)
                    .await
                    .into_iter()
                    .filter(|t| t.status != lukan_tools::tasks::TaskStatus::Done)
                    .collect();
            }
        } else if key.code == KeyCode::Char('s')
            && key.modifiers.contains(crossterm::event::KeyModifiers::ALT)
        {
            // Alt+S: toggle subagent picker
            if self.subagent_picker.is_some() {
                self.subagent_picker = None;
                self.force_redraw = true;
            } else {
                let agents = get_all_sub_agents().await;
                if agents.is_empty() {
                    self.messages
                        .push(ChatMessage::new("system", "No subagents running."));
                } else {
                    let entries: Vec<SubAgentDisplayEntry> = agents
                        .iter()
                        .map(|a| {
                            let elapsed =
                                if a.status == lukan_agent::sub_agent::SubAgentStatus::Running {
                                    let secs = chrono::Utc::now()
                                        .signed_duration_since(a.started_at)
                                        .num_seconds();
                                    format!("{secs}s running")
                                } else {
                                    a.completed_at
                                        .map(|c| {
                                            let secs =
                                                c.signed_duration_since(a.started_at).num_seconds();
                                            format!("{secs}s")
                                        })
                                        .unwrap_or_else(|| "?".to_string())
                                };
                            let task_preview = if a.task.len() > 60 {
                                let end = a.task.floor_char_boundary(57);
                                format!("{}...", &a.task[..end])
                            } else {
                                a.task.clone()
                            };
                            SubAgentDisplayEntry {
                                id: a.id.clone(),
                                task: task_preview,
                                status: format!("{}", a.status),
                                turns: format!("{}", a.turns),
                                elapsed,
                            }
                        })
                        .collect();
                    self.subagent_picker = Some(SubAgentPicker::new(entries));
                }
            }
        } else if key.code == KeyCode::Char('e')
            && key.modifiers.contains(crossterm::event::KeyModifiers::ALT)
        {
            // Alt+E: toggle between Main and Event Agent views
            // Close event picker if open (return events to buffer)
            if let Some(ref mut picker) = self.event_picker {
                let returned = picker.return_all();
                self.event_buffer.extend(returned);
                self.event_picker = None;
            }
            match self.active_view {
                ActiveView::Main => {
                    self.active_view = ActiveView::EventAgent;
                    self.event_agent_has_unread = false;
                    if self.event_messages.is_empty() {
                        self.event_messages.push(ChatMessage::new(
                            "system",
                            "Event Agent view. System events will appear here.\nAlt+L to view events. Press Alt+E to return to main view.",
                        ));
                    }
                }
                ActiveView::EventAgent => {
                    self.active_view = ActiveView::Main;
                }
            }
            self.force_redraw = true;
        } else if key.code == KeyCode::Char('l')
            && key.modifiers.contains(crossterm::event::KeyModifiers::ALT)
            && self.active_view == ActiveView::EventAgent
        {
            // Alt+L: open/close unified event view (Event Agent view only)
            if self.event_picker.is_some() {
                // Close — return pending to buffer if picker mode
                if let Some(ref mut picker) = self.event_picker
                    && picker.mode == EventPickerMode::Picker
                {
                    let returned = picker.return_all();
                    self.event_buffer.extend(returned);
                }
                self.event_picker = None;
            } else if !self.event_buffer.is_empty() {
                // Picker mode — pending events exist
                let events: Vec<_> = self.event_buffer.drain(..).collect();
                self.event_picker = Some(EventPicker::new_picker(events));
            } else {
                // No pending events — load history into picker so user can re-send
                let history = Self::load_event_history(50);
                if history.is_empty() {
                    self.event_picker = Some(EventPicker::new_log(history));
                } else {
                    // Convert history (ts, level, source, detail) → picker entries (source, level, detail)
                    let events: Vec<_> = history
                        .into_iter()
                        .map(|(_ts, level, source, detail)| (source, level, detail))
                        .collect();
                    self.event_picker = Some(EventPicker::new_picker(events));
                }
            }
            self.force_redraw = true;
        } else if key.code == KeyCode::Char('a')
            && key.modifiers.contains(crossterm::event::KeyModifiers::ALT)
            && self.event_picker.is_none()
        {
            // Alt+A: toggle auto/manual event forwarding mode
            self.event_auto_mode = !self.event_auto_mode;
            let mode_label = if self.event_auto_mode {
                "AUTO"
            } else {
                "MANUAL"
            };
            let msgs = match self.active_view {
                ActiveView::Main => &mut self.messages,
                ActiveView::EventAgent => &mut self.event_messages,
            };
            msgs.push(ChatMessage::new(
                "system",
                format!("Event forwarding mode: {mode_label}"),
            ));
            self.force_redraw = true;
        } else if key.code == KeyCode::BackTab {
            // Shift+Tab: cycle permission mode (works during streaming too)
            self.permission_mode = self.permission_mode.next();
            if let Some(ref daemon) = self.daemon_tx {
                let _ = daemon.send(&crate::ws_client::OutMessage::SetPermissionMode {
                    mode: self.permission_mode.to_string(),
                });
            } else if let Some(ref mut agent) = self.agent {
                agent.set_permission_mode(self.permission_mode.clone());
            }
            self.messages.push(ChatMessage::new(
                "system",
                format!("Permission mode: {}", self.permission_mode),
            ));
        } else if !self.is_streaming {
            let cmds = filtered_commands(&self.input);
            let has_palette =
                !cmds.is_empty() && self.session_picker.is_none() && self.model_picker.is_none();

            match key.code {
                KeyCode::Up if has_palette => {
                    if self.cmd_palette_idx > 0 {
                        self.cmd_palette_idx -= 1;
                    } else {
                        self.cmd_palette_idx = cmds.len().saturating_sub(1);
                    }
                }
                KeyCode::Down if has_palette => {
                    self.cmd_palette_idx = (self.cmd_palette_idx + 1) % cmds.len().max(1);
                }
                KeyCode::Up if !has_palette && !self.input.contains('\n') => {
                    let count = self.messages.iter().filter(|m| m.role == "user").count();
                    if count > 0 {
                        match self.history_idx {
                            None => {
                                self.history_saved_input = self.input.clone();
                                self.history_idx = Some(count - 1);
                            }
                            Some(idx) if idx > 0 => {
                                self.history_idx = Some(idx - 1);
                            }
                            _ => {}
                        }
                        if let Some(idx) = self.history_idx
                            && let Some(msg) =
                                self.messages.iter().filter(|m| m.role == "user").nth(idx)
                        {
                            self.input = msg.content.clone();
                            self.cursor_pos = self.input.len();
                            self.paste_info = None;
                        }
                    }
                }
                KeyCode::Down if !has_palette && self.history_idx.is_some() => {
                    let count = self.messages.iter().filter(|m| m.role == "user").count();
                    let idx = self.history_idx.unwrap();
                    if idx + 1 < count {
                        self.history_idx = Some(idx + 1);
                        if let Some(msg) = self
                            .messages
                            .iter()
                            .filter(|m| m.role == "user")
                            .nth(idx + 1)
                        {
                            self.input = msg.content.clone();
                            self.cursor_pos = self.input.len();
                        }
                    } else {
                        self.history_idx = None;
                        self.input = self.history_saved_input.clone();
                        self.cursor_pos = self.input.len();
                    }
                    self.paste_info = None;
                }
                KeyCode::Esc => {
                    if has_palette && !self.esc_pending {
                        self.input.clear();
                        self.cursor_pos = 0;
                        self.cmd_palette_idx = 0;
                        self.paste_info = None;
                    } else if self.esc_pending {
                        self.input.clear();
                        self.cursor_pos = 0;
                        self.cmd_palette_idx = 0;
                        self.esc_pending = false;
                        self.paste_info = None;
                    } else if !self.input.is_empty() {
                        self.esc_pending = true;
                    }
                }
                KeyCode::Enter => {
                    if has_palette {
                        let idx = self.cmd_palette_idx.min(cmds.len().saturating_sub(1));
                        self.input = cmds[idx].0.to_string();
                        self.cursor_pos = self.input.len();
                        self.cmd_palette_idx = 0;
                    }
                    if !self.input.trim().is_empty() {
                        match self.active_view {
                            ActiveView::Main => {
                                self.submit_message(agent_tx.clone()).await;
                            }
                            ActiveView::EventAgent => {
                                self.submit_to_event_agent(event_agent_tx.clone()).await;
                            }
                        }
                    }
                }
                KeyCode::Char(c) => {
                    let clen = c.len_utf8();
                    self.input.insert(self.cursor_pos, c);
                    // Shift paste boundaries if inserting before/at start
                    if let Some((ref mut s, ref mut e, _)) = self.paste_info
                        && self.cursor_pos <= *s
                    {
                        *s += clen;
                        *e += clen;
                    }
                    self.cursor_pos += clen;
                    self.cmd_palette_idx = 0;
                    self.esc_pending = false;
                }
                KeyCode::Backspace => {
                    if let Some((ps, pe, _)) = self.paste_info {
                        if self.cursor_pos == pe {
                            // At paste end → delete entire paste block
                            self.input.drain(ps..pe);
                            self.cursor_pos = ps;
                            self.paste_info = None;
                        } else if self.cursor_pos > pe && self.cursor_pos > 0 {
                            // After paste block — normal delete
                            let prev = self.input[..self.cursor_pos]
                                .char_indices()
                                .next_back()
                                .map(|(i, _)| i)
                                .unwrap_or(0);
                            self.input.drain(prev..self.cursor_pos);
                            self.cursor_pos = prev;
                        } else if self.cursor_pos <= ps && self.cursor_pos > 0 {
                            // Before paste block — normal delete, shift paste
                            let prev = self.input[..self.cursor_pos]
                                .char_indices()
                                .next_back()
                                .map(|(i, _)| i)
                                .unwrap_or(0);
                            let removed = self.cursor_pos - prev;
                            self.input.drain(prev..self.cursor_pos);
                            self.cursor_pos = prev;
                            if let Some((ref mut s, ref mut e, _)) = self.paste_info {
                                *s -= removed;
                                *e -= removed;
                            }
                        }
                    } else if self.cursor_pos > 0 {
                        let prev = self.input[..self.cursor_pos]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                        self.input.drain(prev..self.cursor_pos);
                        self.cursor_pos = prev;
                    }
                    self.cmd_palette_idx = 0;
                    self.esc_pending = false;
                }
                KeyCode::Left if self.cursor_pos > 0 => {
                    if let Some((ps, pe, _)) = self.paste_info
                        && self.cursor_pos > ps
                        && self.cursor_pos <= pe
                    {
                        // Jump over paste block
                        self.cursor_pos = ps;
                    } else {
                        self.cursor_pos = self.input[..self.cursor_pos]
                            .char_indices()
                            .next_back()
                            .map(|(i, _)| i)
                            .unwrap_or(0);
                    }
                }
                KeyCode::Right if self.cursor_pos < self.input.len() => {
                    if let Some((ps, pe, _)) = self.paste_info
                        && self.cursor_pos >= ps
                        && self.cursor_pos < pe
                    {
                        // Jump over paste block
                        self.cursor_pos = pe;
                    } else {
                        self.cursor_pos = self.input[self.cursor_pos..]
                            .char_indices()
                            .nth(1)
                            .map(|(i, _)| self.cursor_pos + i)
                            .unwrap_or(self.input.len());
                    }
                }
                KeyCode::Home => self.cursor_pos = 0,
                KeyCode::End => self.cursor_pos = self.input.len(),
                _ => {}
            }
        } else if self.is_streaming {
            // Typing keys during streaming (Enter/Esc handled above)
            match key.code {
                KeyCode::Char(c) => {
                    let clen = c.len_utf8();
                    self.input.insert(self.cursor_pos, c);
                    if let Some((ref mut s, ref mut e, _)) = self.paste_info
                        && self.cursor_pos <= *s
                    {
                        *s += clen;
                        *e += clen;
                    }
                    self.cursor_pos += clen;
                }
                KeyCode::Backspace if self.cursor_pos > 0 => {
                    let prev = self.input[..self.cursor_pos]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input.drain(prev..self.cursor_pos);
                    self.cursor_pos = prev;
                }
                KeyCode::Left if self.cursor_pos > 0 => {
                    self.cursor_pos = self.input[..self.cursor_pos]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                KeyCode::Right if self.cursor_pos < self.input.len() => {
                    self.cursor_pos = self.input[self.cursor_pos..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor_pos + i)
                        .unwrap_or(self.input.len());
                }
                KeyCode::Home => self.cursor_pos = 0,
                KeyCode::End => self.cursor_pos = self.input.len(),
                _ => {}
            }
        }
        false
    }
}
