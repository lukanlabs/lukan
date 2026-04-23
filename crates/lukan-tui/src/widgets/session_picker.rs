use chrono::Utc;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget},
};

use crate::app::SessionPicker;

pub(crate) struct SessionPickerWidget<'a> {
    picker: &'a SessionPicker,
}

impl<'a> SessionPickerWidget<'a> {
    pub(crate) fn new(picker: &'a SessionPicker) -> Self {
        Self { picker }
    }
}

impl Widget for SessionPickerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let mut lines: Vec<Line<'_>> = Vec::new();

        lines.push(Line::from(Span::styled(
            " Resume Session",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // Each session takes 1 line
        let available_rows = area.height.saturating_sub(3) as usize; // minus header + blank + scroll indicator
        let visible_items = available_rows.max(1);
        let total = self.picker.sessions.len();
        let selected = self.picker.selected;

        // Scroll offset: keep selected item visible
        let scroll_offset = if selected >= visible_items {
            selected - visible_items + 1
        } else {
            0
        };
        let end = (scroll_offset + visible_items).min(total);

        for i in scroll_offset..end {
            let session = &self.picker.sessions[i];
            let is_selected = i == selected;
            let is_current = self
                .picker
                .current_id
                .as_ref()
                .is_some_and(|id| *id == session.id);

            let pointer = if is_selected { "▸ " } else { "  " };

            let time_ago = format_time_ago(session.updated_at);
            let msg_count = session.message_count;

            let mut spans = vec![
                Span::styled(
                    pointer,
                    if is_selected {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(
                    format!("[{}]", session.id),
                    if is_selected {
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Yellow)
                    },
                ),
                Span::styled(
                    format!(" · {msg_count} msgs · {time_ago}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ];

            if let Some(ref cwd) = session.cwd
                && let Some(project_root) = session.project_root.as_ref()
                && cwd != project_root
                && let Some(name) = std::path::Path::new(cwd)
                    .file_name()
                    .and_then(|n| n.to_str())
            {
                spans.push(Span::styled(
                    format!(" [worktree:{name}]"),
                    Style::default().fg(Color::Green),
                ));
            }

            if is_current {
                spans.push(Span::styled(
                    " (current)",
                    Style::default().fg(Color::Green),
                ));
            }

            // Last message preview on the right, dimmed
            if let Some(ref preview) = session.last_message {
                spans.push(Span::styled(
                    format!(" · {preview}"),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            lines.push(Line::from(spans));
        }

        // Scroll indicator
        if total > visible_items {
            lines.push(Line::from(Span::styled(
                format!("  ({}/{total})", selected + 1),
                Style::default().fg(Color::DarkGray),
            )));
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

/// Format a timestamp as a human-readable "time ago" string
pub(crate) fn format_time_ago(dt: chrono::DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(dt);

    let seconds = duration.num_seconds();
    if seconds < 60 {
        return format!("{seconds}s ago");
    }

    let minutes = duration.num_minutes();
    if minutes < 60 {
        return format!("{minutes}m ago");
    }

    let hours = duration.num_hours();
    if hours < 24 {
        return format!("{hours}h ago");
    }

    let days = duration.num_days();
    if days < 30 {
        return format!("{days}d ago");
    }

    let months = days / 30;
    format!("{months}mo ago")
}
