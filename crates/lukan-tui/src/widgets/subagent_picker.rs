use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget, Wrap},
};

use super::chat::{ChatMessage, build_message_lines, physical_row_count};

/// View mode for the subagent picker
#[derive(Clone, Copy, PartialEq)]
pub enum SubAgentPickerView {
    /// List of all subagents
    List,
    /// Chat-style read-only view of the selected subagent
    ChatDetail,
}

/// A subagent entry for display in the list
pub struct SubAgentDisplayEntry {
    pub id: String,
    pub task: String,
    pub status: String,
    pub turns: String,
    pub elapsed: String,
}

/// SubAgent picker state
pub struct SubAgentPicker {
    pub entries: Vec<SubAgentDisplayEntry>,
    pub selected: usize,
    pub view: SubAgentPickerView,
    // ChatDetail state
    pub detail_id: String,
    pub detail_status: String,
    pub detail_turns: String,
    pub detail_tokens: String,
    pub detail_error: Option<String>,
    /// Chat messages from the subagent conversation (rendered like the main chat)
    pub detail_messages: Vec<ChatMessage>,
    pub scroll_offset: u16,
}

impl SubAgentPicker {
    pub fn new(entries: Vec<SubAgentDisplayEntry>) -> Self {
        Self {
            entries,
            selected: 0,
            view: SubAgentPickerView::List,
            detail_id: String::new(),
            detail_status: String::new(),
            detail_turns: String::new(),
            detail_tokens: String::new(),
            detail_error: None,
            detail_messages: Vec::new(),
            scroll_offset: 0,
        }
    }

    pub fn selected_entry(&self) -> Option<&SubAgentDisplayEntry> {
        self.entries.get(self.selected)
    }
}

/// Widget that renders the SubAgentPicker
pub struct SubAgentPickerWidget<'a> {
    picker: &'a SubAgentPicker,
}

impl<'a> SubAgentPickerWidget<'a> {
    pub fn new(picker: &'a SubAgentPicker) -> Self {
        Self { picker }
    }
}

impl Widget for SubAgentPickerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        match self.picker.view {
            SubAgentPickerView::List => render_list(self.picker, area, buf),
            SubAgentPickerView::ChatDetail => render_chat_detail(self.picker, area, buf),
        }
    }
}

fn render_list(picker: &SubAgentPicker, area: Rect, buf: &mut Buffer) {
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled(
            " Subagents",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  (↑↓ navigate · Enter view · ESC close)",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(""));

    if picker.entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No subagents found.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let available_rows = area.height.saturating_sub(4) as usize;
        let visible_items = available_rows.max(1);
        let total = picker.entries.len();
        let selected = picker.selected;

        let scroll_offset = if selected >= visible_items {
            selected - visible_items + 1
        } else {
            0
        };
        let end = (scroll_offset + visible_items).min(total);

        for i in scroll_offset..end {
            let entry = &picker.entries[i];
            let is_selected = i == selected;

            let pointer = if is_selected { "▸ " } else { "  " };

            let (status_icon, status_color) = match entry.status.as_str() {
                "running" => ("●", Color::Yellow),
                "completed" => ("✓", Color::Green),
                "error" => ("✗", Color::Red),
                "aborted" => ("⊘", Color::DarkGray),
                _ => ("?", Color::DarkGray),
            };

            let pointer_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let id_style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            };

            let task_style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            lines.push(Line::from(vec![
                Span::styled(pointer, pointer_style),
                Span::styled(status_icon, Style::default().fg(status_color)),
                Span::styled(format!(" {}", entry.id), id_style),
                Span::styled(format!(" {}", entry.task), task_style),
                Span::styled(
                    format!("  {} turns", entry.turns),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("  {}", entry.elapsed),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }

        // Scroll indicator
        if total > visible_items {
            lines.push(Line::from(Span::styled(
                format!("  ({}/{total})", selected + 1),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    paragraph.render(area, buf);
}

fn render_chat_detail(picker: &SubAgentPicker, area: Rect, buf: &mut Buffer) {
    // Header bar (2 lines: header + blank)
    let status_color = match picker.detail_status.as_str() {
        "running" => Color::Yellow,
        "completed" => Color::Green,
        "error" => Color::Red,
        "aborted" => Color::DarkGray,
        _ => Color::DarkGray,
    };

    let header_lines: Vec<Line<'_>> = vec![
        Line::from(vec![
            Span::styled(
                format!(" Agent {}", picker.detail_id),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" — {}", picker.detail_status),
                Style::default().fg(status_color),
            ),
            Span::styled(
                format!("  {} turns", picker.detail_turns),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("  {}", picker.detail_tokens),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                "  (↑↓ scroll · ESC back)",
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(""),
    ];

    // Render header in top area
    let header_h = 2u16;
    let header_area = Rect {
        height: header_h.min(area.height),
        ..area
    };
    let header_para = Paragraph::new(header_lines);
    header_para.render(header_area, buf);

    // Chat area below header
    if area.height <= header_h {
        return;
    }
    let chat_area = Rect {
        y: area.y + header_h,
        height: area.height - header_h,
        ..area
    };

    // Build streaming indicator for running agents
    let streaming = if picker.detail_status == "running" {
        "\n● Agent is working..."
    } else if picker.detail_status == "error" {
        let err = picker.detail_error.as_deref().unwrap_or("unknown error");
        // We can't easily build a dynamic string for streaming, so add as a message below
        let _ = err;
        ""
    } else {
        ""
    };

    // Build chat lines using the same renderer as the main chat
    let mut messages = picker.detail_messages.clone();

    // Add error as system message at the end
    if picker.detail_status == "error" {
        let err = picker.detail_error.as_deref().unwrap_or("unknown error");
        messages.push(ChatMessage::new("system", format!("✗ Error: {err}")));
    }

    let lines = build_message_lines(&messages, "", streaming);

    // Render with scroll support
    let total_rows = physical_row_count(&lines, chat_area.width);

    if total_rows <= chat_area.height {
        // Content fits — render at bottom (same as main chat)
        let top_padding = chat_area.height - total_rows;
        let bottom_area = Rect {
            y: chat_area.y + top_padding,
            height: total_rows.max(1),
            ..chat_area
        };
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        paragraph.render(bottom_area, buf);
    } else {
        // Scrollable — default to bottom, scroll_offset moves up
        let max_scroll = total_rows - chat_area.height;
        let scroll = max_scroll.saturating_sub(picker.scroll_offset);
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0));
        paragraph.render(chat_area, buf);
    }
}
