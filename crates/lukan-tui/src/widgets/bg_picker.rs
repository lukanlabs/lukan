use chrono::{DateTime, Utc};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget, Wrap},
};

/// View mode for the background process picker
#[derive(Clone, Copy, PartialEq)]
pub enum BgPickerView {
    /// List of processes
    List,
    /// Log output for a specific PID
    Log,
}

/// A single background process entry for display
pub struct BgEntry {
    pub pid: u32,
    pub command: String,
    pub started_at: DateTime<Utc>,
    pub alive: bool,
}

/// Background process picker state
pub struct BgPicker {
    pub entries: Vec<BgEntry>,
    pub selected: usize,
    pub view: BgPickerView,
    /// Cached log content for the currently viewed process
    pub log_content: String,
    /// PID of the process whose log is being viewed
    pub log_pid: u32,
}

impl BgPicker {
    pub fn new(entries: Vec<BgEntry>) -> Self {
        Self {
            entries,
            selected: 0,
            view: BgPickerView::List,
            log_content: String::new(),
            log_pid: 0,
        }
    }

    pub fn selected_pid(&self) -> Option<u32> {
        self.entries.get(self.selected).map(|e| e.pid)
    }

    /// Refresh entries from the bg_processes tracker
    pub fn refresh(&mut self) {
        let processes = lukan_tools::bg_processes::get_bg_processes();
        self.entries = processes
            .into_iter()
            .map(|(pid, command, started_at, alive)| BgEntry {
                pid,
                command,
                started_at,
                alive,
            })
            .collect();
        // Clamp selection
        if self.selected >= self.entries.len() && !self.entries.is_empty() {
            self.selected = self.entries.len() - 1;
        }
    }

    /// Load log for the selected process
    pub fn load_log(&mut self) {
        if let Some(pid) = self.selected_pid() {
            self.log_pid = pid;
            self.log_content = lukan_tools::bg_processes::get_bg_log(pid, 200)
                .unwrap_or_else(|| "(no log output yet)".to_string());
            self.view = BgPickerView::Log;
        }
    }

    /// Refresh the log content if in log view
    pub fn refresh_log(&mut self) {
        if self.view == BgPickerView::Log && self.log_pid > 0 {
            self.log_content = lukan_tools::bg_processes::get_bg_log(self.log_pid, 200)
                .unwrap_or_else(|| "(no log output yet)".to_string());
        }
    }
}

/// Widget that renders the BgPicker
pub struct BgPickerWidget<'a> {
    picker: &'a BgPicker,
}

impl<'a> BgPickerWidget<'a> {
    pub fn new(picker: &'a BgPicker) -> Self {
        Self { picker }
    }
}

impl Widget for BgPickerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        match self.picker.view {
            BgPickerView::List => render_list(self.picker, area, buf),
            BgPickerView::Log => render_log(self.picker, area, buf),
        }
    }
}

fn render_list(picker: &BgPicker, area: Rect, buf: &mut Buffer) {
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled(
            " Background Processes",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  (l=logs  k=kill  ESC=close)",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(""));

    if picker.entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No background processes.",
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

            let status = if entry.alive { "●" } else { "○" };
            let status_color = if entry.alive {
                Color::Green
            } else {
                Color::DarkGray
            };

            let uptime = format_uptime(entry.started_at);

            // Truncate command to fit
            let max_cmd_len = (area.width as usize).saturating_sub(30);
            let cmd_display = if entry.command.len() > max_cmd_len {
                format!("{}…", &entry.command[..max_cmd_len.saturating_sub(1)])
            } else {
                entry.command.clone()
            };

            let pointer_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let pid_style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            };

            let cmd_style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            lines.push(Line::from(vec![
                Span::styled(pointer, pointer_style),
                Span::styled(status, Style::default().fg(status_color)),
                Span::styled(format!(" PID:{:<6}", entry.pid), pid_style),
                Span::styled(format!(" {cmd_display}"), cmd_style),
                Span::styled(format!("  {uptime}"), Style::default().fg(Color::DarkGray)),
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

fn render_log(picker: &BgPicker, area: Rect, buf: &mut Buffer) {
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Header
    let alive = lukan_tools::bg_processes::is_process_alive(picker.log_pid);
    let status = if alive { "● running" } else { "○ finished" };
    let status_color = if alive { Color::Green } else { Color::DarkGray };

    lines.push(Line::from(vec![
        Span::styled(
            format!(" PID:{} ", picker.log_pid),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(status, Style::default().fg(status_color)),
        Span::styled(
            "  (ESC=back  k=kill)",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(""));

    // Log content
    if picker.log_content.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no output yet)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let available_rows = area.height.saturating_sub(3) as usize;
        let log_lines: Vec<&str> = picker.log_content.lines().collect();
        let start = if log_lines.len() > available_rows {
            log_lines.len() - available_rows
        } else {
            0
        };
        for line in &log_lines[start..] {
            lines.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(Color::Gray),
            )));
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    paragraph.render(area, buf);
}

/// Format uptime from start time to now
fn format_uptime(started_at: DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(started_at);
    let seconds = duration.num_seconds();

    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{}m", seconds / 3600, (seconds % 3600) / 60)
    }
}
