use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget, Wrap},
};

/// View mode for the worker picker
#[derive(Clone, Copy, PartialEq)]
pub enum WorkerPickerView {
    /// List of all workers
    WorkerList,
    /// Runs for the selected worker
    RunList,
    /// Full output of a selected run
    RunDetail,
}

/// A worker entry for display in the list
pub struct WorkerEntry {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: String,
    pub last_run_status: Option<String>,
}

/// A worker run entry for display in the run list
pub struct RunEntry {
    pub id: String,
    pub status: String,
    pub started_at: String,
    pub turns: u32,
}

/// Worker picker state
pub struct WorkerPicker {
    pub entries: Vec<WorkerEntry>,
    pub selected: usize,
    pub view: WorkerPickerView,
    // RunList state
    pub runs: Vec<RunEntry>,
    pub run_selected: usize,
    pub selected_worker_name: String,
    pub selected_worker_id: String,
    // RunDetail state
    pub run_output: String,
    pub run_status: String,
    pub run_id: String,
}

impl WorkerPicker {
    pub fn new(entries: Vec<WorkerEntry>) -> Self {
        Self {
            entries,
            selected: 0,
            view: WorkerPickerView::WorkerList,
            runs: Vec::new(),
            run_selected: 0,
            selected_worker_name: String::new(),
            selected_worker_id: String::new(),
            run_output: String::new(),
            run_status: String::new(),
            run_id: String::new(),
        }
    }

    pub fn selected_worker(&self) -> Option<&WorkerEntry> {
        self.entries.get(self.selected)
    }

    pub fn selected_run(&self) -> Option<&RunEntry> {
        self.runs.get(self.run_selected)
    }
}

/// Widget that renders the WorkerPicker
pub struct WorkerPickerWidget<'a> {
    picker: &'a WorkerPicker,
}

impl<'a> WorkerPickerWidget<'a> {
    pub fn new(picker: &'a WorkerPicker) -> Self {
        Self { picker }
    }
}

impl Widget for WorkerPickerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        match self.picker.view {
            WorkerPickerView::WorkerList => render_worker_list(self.picker, area, buf),
            WorkerPickerView::RunList => render_run_list(self.picker, area, buf),
            WorkerPickerView::RunDetail => render_run_detail(self.picker, area, buf),
        }
    }
}

fn render_worker_list(picker: &WorkerPicker, area: Rect, buf: &mut Buffer) {
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled(
            " Workers",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  (↑↓ navigate · Enter runs · ESC close)",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(""));

    if picker.entries.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No workers configured.",
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

            let status_icon = if entry.enabled { "●" } else { "○" };
            let status_color = if entry.enabled {
                Color::Green
            } else {
                Color::DarkGray
            };

            let last_run = entry.last_run_status.as_deref().unwrap_or("never run");
            let last_color = match last_run {
                "success" => Color::Green,
                "error" => Color::Red,
                _ => Color::DarkGray,
            };

            let pointer_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let name_style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            lines.push(Line::from(vec![
                Span::styled(pointer, pointer_style),
                Span::styled(status_icon, Style::default().fg(status_color)),
                Span::styled(format!(" {}", entry.name), name_style),
                Span::styled(
                    format!("  sched={}", entry.schedule),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("  last={last_run}"),
                    Style::default().fg(last_color),
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

fn render_run_list(picker: &WorkerPicker, area: Rect, buf: &mut Buffer) {
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {} — Runs", picker.selected_worker_name),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  (↑↓ navigate · Enter detail · ESC back)",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(""));

    if picker.runs.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No runs recorded.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let available_rows = area.height.saturating_sub(4) as usize;
        let visible_items = available_rows.max(1);
        let total = picker.runs.len();
        let selected = picker.run_selected;

        let scroll_offset = if selected >= visible_items {
            selected - visible_items + 1
        } else {
            0
        };
        let end = (scroll_offset + visible_items).min(total);

        for i in scroll_offset..end {
            let run = &picker.runs[i];
            let is_selected = i == selected;

            let pointer = if is_selected { "▸ " } else { "  " };

            let status_icon = match run.status.as_str() {
                "success" => "✓",
                "error" => "✗",
                "running" => "●",
                _ => "?",
            };
            let status_color = match run.status.as_str() {
                "success" => Color::Green,
                "error" => Color::Red,
                "running" => Color::Yellow,
                _ => Color::DarkGray,
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

            // Show short ID (first 8 chars)
            let short_id = if run.id.len() > 8 {
                &run.id[..8]
            } else {
                &run.id
            };

            // Format timestamp — show just date+time part
            let time_display = if run.started_at.len() > 19 {
                &run.started_at[..19]
            } else {
                &run.started_at
            };

            lines.push(Line::from(vec![
                Span::styled(pointer, pointer_style),
                Span::styled(status_icon, Style::default().fg(status_color)),
                Span::styled(format!(" {short_id}"), id_style),
                Span::styled(
                    format!("  {time_display}"),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("  {} turns", run.turns),
                    Style::default().fg(Color::Gray),
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

fn render_run_detail(picker: &WorkerPicker, area: Rect, buf: &mut Buffer) {
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Header
    let status_color = match picker.run_status.as_str() {
        "success" => Color::Green,
        "error" => Color::Red,
        "running" => Color::Yellow,
        _ => Color::DarkGray,
    };

    let short_id = if picker.run_id.len() > 8 {
        &picker.run_id[..8]
    } else {
        &picker.run_id
    };

    lines.push(Line::from(vec![
        Span::styled(
            format!(" Run {short_id}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" — {}", picker.run_status),
            Style::default().fg(status_color),
        ),
        Span::styled("  (ESC back)", Style::default().fg(Color::DarkGray)),
    ]));
    lines.push(Line::from(""));

    // Output content (scrollable — show tail)
    if picker.run_output.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no output)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        let available_rows = area.height.saturating_sub(3) as usize;
        let output_lines: Vec<&str> = picker.run_output.lines().collect();
        let start = if output_lines.len() > available_rows {
            output_lines.len() - available_rows
        } else {
            0
        };
        for line in &output_lines[start..] {
            lines.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(Color::Gray),
            )));
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    paragraph.render(area, buf);
}
