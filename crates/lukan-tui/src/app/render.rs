use super::*;

impl App {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_frame(
        &mut self,
        frame: &mut ratatui::Frame,
        palette_visible: bool,
        palette_h: u16,
        palette_idx: usize,
        filtered_cmds: &[(&'static str, &'static str)],
        input_h: u16,
        task_panel_h: u16,
    ) {
        let area = frame.area();

        // Shimmer status line: 1 row when streaming (in active view), 0 otherwise
        let view_is_streaming = match self.active_view {
            ActiveView::Main => self.is_streaming,
            ActiveView::EventAgent => self.event_is_streaming,
        };
        let shimmer_h: u16 = if view_is_streaming { 1 } else { 0 };

        // Queued message indicator: 1 row when a message is queued
        let queued_h: u16 = self.queued_messages.lock().unwrap().len() as u16;

        // Dynamic layout: palette below input, above status bar
        // margin_h adds a 1-row gap between chat content and shimmer/input
        let margin_h: u16 = 1;
        let (
            chat_area,
            task_panel_area,
            shimmer_area,
            queued_area,
            input_area,
            palette_area,
            status_area,
        ) = if palette_visible {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(task_panel_h),
                    Constraint::Length(margin_h),
                    Constraint::Length(shimmer_h),
                    Constraint::Length(queued_h),
                    Constraint::Length(input_h),
                    Constraint::Length(palette_h),
                    Constraint::Length(1),
                ])
                .split(area);
            (
                chunks[0],
                chunks[1],
                chunks[3],
                chunks[4],
                chunks[5],
                Some(chunks[6]),
                chunks[7],
            )
        } else {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(1),
                    Constraint::Length(task_panel_h),
                    Constraint::Length(margin_h),
                    Constraint::Length(shimmer_h),
                    Constraint::Length(queued_h),
                    Constraint::Length(input_h),
                    Constraint::Length(1),
                ])
                .split(area);
            (
                chunks[0], chunks[1], chunks[3], chunks[4], chunks[5], None, chunks[6],
            )
        };

        // Chat (or overlay pickers)
        if let Some(ref prompt) = self.trust_prompt {
            let widget = TrustPromptWidget::new(prompt);
            frame.render_widget(widget, chat_area);
        } else if let Some(ref state) = self.plan_review {
            let widget = PlanReviewWidget::new(state);
            frame.render_widget(widget, chat_area);
        } else if let Some(ref state) = self.planner_question {
            let widget = PlannerQuestionWidget::new(state);
            frame.render_widget(widget, chat_area);
        } else if let Some(ref prompt) = self.approval_prompt {
            let widget = ApprovalPromptWidget::new(prompt);
            frame.render_widget(widget, chat_area);
        } else if let Some(ref content) = self.memory_viewer {
            use ratatui::widgets::{Block, Borders, Wrap};
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Memory (↑↓ PgUp PgDown Home End to scroll, ESC to close) ")
                .border_style(Style::default().fg(Color::Cyan));
            let paragraph = ratatui::widgets::Paragraph::new(content.as_str())
                .block(block)
                .wrap(Wrap { trim: false })
                .scroll((self.memory_viewer_scroll, 0))
                .style(Style::default().fg(Color::White));
            frame.render_widget(paragraph, chat_area);
        } else if let Some(ref picker) = self.rewind_picker {
            let widget = RewindPickerWidget::new(picker);
            frame.render_widget(widget, chat_area);
        } else if let Some(ref picker) = self.bg_picker {
            let widget = BgPickerWidget::new(picker);
            frame.render_widget(widget, chat_area);
        } else if let Some(ref picker) = self.worker_picker {
            let widget = WorkerPickerWidget::new(picker);
            frame.render_widget(widget, chat_area);
        } else if let Some(ref picker) = self.subagent_picker {
            let widget = SubAgentPickerWidget::new(picker);
            frame.render_widget(widget, chat_area);
        } else if let Some(ref picker) = self.session_picker {
            let widget = SessionPickerWidget::new(picker);
            frame.render_widget(widget, chat_area);
        } else if let Some(ref picker) = self.event_picker {
            let widget = EventPickerWidget::new(picker);
            frame.render_widget(widget, chat_area);
        } else {
            // Add left margin so chat text doesn't hug the border
            let padded_chat = Rect {
                x: chat_area.x + 1,
                width: chat_area.width.saturating_sub(1),
                ..chat_area
            };
            // Show messages/streaming based on active view
            let (msgs, committed_idx, streaming, vscroll) = match self.active_view {
                ActiveView::Main => (
                    &self.messages,
                    self.committed_msg_idx,
                    &self.streaming_text,
                    self.viewport_scroll,
                ),
                ActiveView::EventAgent => (
                    &self.event_messages,
                    self.event_committed_msg_idx,
                    &self.event_streaming_text,
                    self.event_viewport_scroll,
                ),
            };
            let has_scrollback = committed_idx > 0 || vscroll > 0;
            let chat = ChatWidget::new(&msgs[committed_idx..], streaming, has_scrollback, vscroll);
            frame.render_widget(chat, padded_chat);
        }

        // Task panel — between chat and shimmer/input
        if self.task_panel_visible && task_panel_area.height > 0 {
            let widget = TaskPanelWidget::new(&self.task_panel_entries);
            frame.render_widget(widget, task_panel_area);
        }

        // Palette area: reasoning picker, model picker, or command palette
        if let Some(p_area) = palette_area {
            if let Some(ref picker) = self.reasoning_picker {
                let widget = ReasoningPaletteWidget::new(picker);
                frame.render_widget(widget, p_area);
            } else if let Some(ref picker) = self.model_picker {
                let widget = ModelPaletteWidget::new(picker);
                frame.render_widget(widget, p_area);
            } else {
                let widget = CommandPaletteWidget::new(filtered_cmds, palette_idx);
                frame.render_widget(widget, p_area);
            }
        }

        // Shimmer indicator — fixed above input
        if view_is_streaming && shimmer_area.height > 0 {
            use crate::widgets::shimmer::shimmer_spans;
            let label = match self.active_view {
                ActiveView::Main => {
                    if self.streaming_text.is_empty() && self.streaming_thinking.is_empty() {
                        "Working on it..."
                    } else if !self.streaming_thinking.is_empty() && self.streaming_text.is_empty()
                    {
                        "Thinking..."
                    } else {
                        "Writing..."
                    }
                }
                ActiveView::EventAgent => {
                    if self.event_streaming_text.is_empty() {
                        "Event Agent working..."
                    } else {
                        "Event Agent writing..."
                    }
                }
            };
            // Match chat content left padding so shimmer text aligns with messages
            let padded_shimmer = Rect {
                x: shimmer_area.x + 1,
                width: shimmer_area.width.saturating_sub(1),
                ..shimmer_area
            };
            let line = ratatui::text::Line::from(shimmer_spans(label));
            let paragraph = ratatui::widgets::Paragraph::new(line);
            frame.render_widget(paragraph, padded_shimmer);
        }

        // Queued message indicator (one line per queued message)
        {
            let queue = self.queued_messages.lock().unwrap();
            if !queue.is_empty() {
                let lines: Vec<ratatui::text::Line> = queue
                    .iter()
                    .enumerate()
                    .map(|(i, msg)| {
                        let label = format!("↳ queued {}: ", i + 1);
                        let max_chars =
                            queued_area.width.saturating_sub(label.len() as u16) as usize;
                        let preview: String = if msg.chars().count() > max_chars {
                            let truncated: String =
                                msg.chars().take(max_chars.saturating_sub(1)).collect();
                            format!("{truncated}…")
                        } else {
                            msg.clone()
                        };
                        ratatui::text::Line::from(vec![
                            ratatui::text::Span::styled(
                                label,
                                Style::default().fg(Color::DarkGray),
                            ),
                            ratatui::text::Span::styled(
                                preview,
                                Style::default().fg(Color::Yellow),
                            ),
                        ])
                    })
                    .collect();
                let paragraph = ratatui::widgets::Paragraph::new(lines);
                frame.render_widget(paragraph, queued_area);
            }
        }

        // Input — show paste preview + typed-after text when available
        let di = self.display_input();
        let dc = self.display_cursor();
        let input_widget = if self.plan_review.is_some() {
            let hint = match self.plan_review.as_ref().map(|p| p.mode) {
                Some(PlanReviewMode::List) => {
                    "↑↓ navigate · Enter view · a accept · r request changes · Esc reject"
                }
                Some(PlanReviewMode::Detail) => "Esc back to list",
                Some(PlanReviewMode::Feedback) => "Type feedback · Enter submit · Esc cancel",
                None => "",
            };
            InputWidget::new(hint, 0, false)
        } else if self.planner_question.is_some() {
            InputWidget::new(
                "↑↓ select · Space toggle · Enter confirm · Tab next question",
                0,
                false,
            )
        } else if self.approval_prompt.is_some() {
            InputWidget::new(
                "Space toggle · Enter submit · a approve all · A always allow · Esc deny all",
                0,
                false,
            )
        } else if self.trust_prompt.is_some() {
            InputWidget::new("↑↓ select · Enter confirm · ESC exit", 0, false)
        } else if self.memory_viewer.is_some() {
            InputWidget::new("↑↓ PgUp PgDown Home End · ESC close", 0, false)
        } else if self.rewind_picker.is_some() {
            let hint = match self.rewind_picker.as_ref().map(|p| p.view) {
                Some(RewindView::List) => "↑↓ navigate · Enter restore · ESC close",
                Some(RewindView::Options) => "↑↓ navigate · Enter confirm · ESC back",
                None => "",
            };
            InputWidget::new(hint, 0, false)
        } else if self.bg_picker.is_some() {
            let hint = match self.bg_picker.as_ref().map(|p| p.view) {
                Some(BgPickerView::List) => "↑↓ navigate · l=logs · k=kill · ESC close",
                Some(BgPickerView::Log) => "ESC=back · k=kill",
                None => "",
            };
            InputWidget::new(hint, 0, false)
        } else if self.worker_picker.is_some() {
            let hint = match self.worker_picker.as_ref().map(|p| p.view) {
                Some(WorkerPickerView::WorkerList) => "↑↓ navigate · Enter runs · ESC close",
                Some(WorkerPickerView::RunList) => "↑↓ navigate · Enter detail · ESC back",
                Some(WorkerPickerView::RunDetail) => "ESC back",
                None => "",
            };
            InputWidget::new(hint, 0, false)
        } else if self.subagent_picker.is_some() {
            let hint = match self.subagent_picker.as_ref().map(|p| p.view) {
                Some(SubAgentPickerView::List) => "↑↓ navigate · Enter view · k kill · ESC close",
                Some(SubAgentPickerView::ChatDetail) => "↑↓ scroll · ESC back",
                None => "",
            };
            InputWidget::new(hint, 0, false)
        } else if self.session_picker.is_some()
            || self.model_picker.is_some()
            || self.reasoning_picker.is_some()
        {
            InputWidget::new("↑↓ navigate · Enter select · ESC close", 0, false)
        } else if self.tool_picker.is_some() {
            InputWidget::new("↑↓ navigate · Space toggle · Esc/Alt+P close", 0, false)
        } else if let Some(ref picker) = self.event_picker {
            match picker.mode {
                EventPickerMode::Picker => InputWidget::new(
                    "←→ tabs · ↑↓ nav · Space toggle · a=all · Enter send · Esc close",
                    0,
                    false,
                ),
                EventPickerMode::Log => {
                    InputWidget::new("←→ tabs · ↑↓ scroll · Esc close", 0, false)
                }
            }
        } else {
            InputWidget::new(&di, dc, true)
        };
        frame.render_widget(input_widget, input_area);

        // ESC hint (rendered inside the input border, right-aligned)
        if self.esc_pending {
            let hint = " ESC to clear ";
            let hint_len = hint.len() as u16;
            if input_area.width > hint_len + 4 {
                let x = input_area.x + input_area.width - hint_len - 1;
                let y = input_area.y + input_area.height.saturating_sub(2); // last content row
                let buf = frame.buffer_mut();
                buf.set_string(x, y, hint, Style::default().fg(Color::DarkGray));
            }
        }

        // Status bar — show correct tokens/tool for the active view
        let effective_model = self
            .config
            .effective_model()
            .unwrap_or_else(|| "(no model)".to_string());
        let memory_active = LukanPaths::project_memory_active_file().exists();
        let mode_str = self.permission_mode.to_string();
        let (
            sb_tokens_in,
            sb_tokens_out,
            sb_cache_read,
            sb_cache_create,
            sb_ctx,
            sb_streaming,
            sb_tool,
        ) = match self.active_view {
            ActiveView::Main => (
                self.input_tokens,
                self.output_tokens,
                self.cache_read_tokens,
                self.cache_creation_tokens,
                self.context_size,
                self.is_streaming,
                self.active_tool.as_deref(),
            ),
            ActiveView::EventAgent => (
                self.event_input_tokens,
                self.event_output_tokens,
                0,
                0,
                0,
                self.event_is_streaming,
                self.event_active_tool.as_deref(),
            ),
        };
        let view_label = match self.active_view {
            ActiveView::Main => None,
            ActiveView::EventAgent => Some(if self.event_auto_mode {
                "Events [AUTO]"
            } else {
                "Events [MANUAL]"
            }),
        };
        let status = StatusBarWidget::new(
            self.provider.name(),
            &effective_model,
            sb_tokens_in,
            sb_tokens_out,
            sb_cache_read,
            sb_cache_create,
            sb_ctx,
            sb_streaming,
            sb_tool,
            memory_active,
            &mode_str,
        )
        .view_label(view_label)
        .event_unread(self.event_agent_has_unread && self.active_view == ActiveView::Main);
        frame.render_widget(status, status_area);

        // Toast notifications — floating overlay in top-right of chat area
        if !self.toast_notifications.is_empty() {
            let toast_count = self.toast_notifications.len().min(5);
            let toasts = &self.toast_notifications[self.toast_notifications.len() - toast_count..];
            let toast_lines: Vec<Line<'_>> = toasts
                .iter()
                .map(|(msg, _)| {
                    Line::from(vec![
                        Span::styled("▸ ", Style::default().fg(Color::Yellow)),
                        Span::styled(msg.clone(), Style::default().fg(Color::DarkGray)),
                    ])
                })
                .collect();
            let toast_h = toast_lines.len() as u16;
            // Find the widest toast line for sizing
            let toast_w = toast_lines
                .iter()
                .map(|l| l.width() as u16 + 2)
                .max()
                .unwrap_or(20)
                .min(chat_area.width);
            let toast_area = Rect {
                x: chat_area.right().saturating_sub(toast_w),
                y: chat_area.y,
                width: toast_w,
                height: toast_h.min(chat_area.height),
            };
            Clear.render(toast_area, frame.buffer_mut());
            let toast_paragraph =
                Paragraph::new(toast_lines).style(Style::default().bg(Color::Rgb(30, 30, 30)));
            frame.render_widget(toast_paragraph, toast_area);
        }

        if let Some(ref picker) = self.tool_picker {
            let overlay_w = (chat_area.width * 80 / 100).max(30);
            let overlay_h = (chat_area.height * 70 / 100).max(8);
            let overlay_x = chat_area.x + (chat_area.width.saturating_sub(overlay_w)) / 2;
            let overlay_y = chat_area.y + (chat_area.height.saturating_sub(overlay_h)) / 2;
            let overlay_area = Rect {
                x: overlay_x,
                y: overlay_y,
                width: overlay_w,
                height: overlay_h,
            };
            Clear.render(overlay_area, frame.buffer_mut());
            let widget = ToolPickerWidget::new(picker);
            frame.render_widget(widget, overlay_area);
        }

        // Embedded terminal overlay (F9) — only when visible
        if self.terminal_visible
            && let Some(ref modal) = self.terminal_modal
        {
            let overlay_w = (chat_area.width * 90 / 100).max(20);
            let overlay_h = (chat_area.height * 85 / 100).max(10);
            let overlay_x = chat_area.x + (chat_area.width.saturating_sub(overlay_w)) / 2;
            let overlay_y = chat_area.y + (chat_area.height.saturating_sub(overlay_h)) / 2;
            let overlay_area = Rect {
                x: overlay_x,
                y: overlay_y,
                width: overlay_w,
                height: overlay_h,
            };
            let widget = TerminalWidget::new(modal.screen(), modal.has_exited())
                .with_selection(modal.selection.as_ref());
            frame.render_widget(widget, overlay_area);
            // Cache inner area for mouse hit-testing (border = 1 on each side)
            self.terminal_overlay_inner = Some(Rect {
                x: overlay_x + 1,
                y: overlay_y + 1,
                width: overlay_w.saturating_sub(2),
                height: overlay_h.saturating_sub(2),
            });
        }

        // Set cursor position only when not in picker/overlay
        if self.trust_prompt.is_none()
            && self.approval_prompt.is_none()
            && self.plan_review.is_none()
            && self.planner_question.is_none()
            && self.memory_viewer.is_none()
            && self.rewind_picker.is_none()
            && self.bg_picker.is_none()
            && self.worker_picker.is_none()
            && self.subagent_picker.is_none()
            && self.session_picker.is_none()
            && self.model_picker.is_none()
            && self.tool_picker.is_none()
            && self.event_picker.is_none()
            && !self.terminal_visible
        {
            let (cx, cy) = cursor_position(&di, dc, input_area);
            frame.set_cursor_position((cx, cy));
        }
    }
}
