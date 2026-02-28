use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget},
};

/// Whether the event picker shows pending events (checkboxes) or historical log
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPickerMode {
    Picker,
    Log,
}

/// A single system event entry for display in the picker (pending, selectable)
pub struct EventEntry {
    pub source: String,
    pub level: String,
    pub detail: String,
    pub selected: bool,
}

/// A single historical log entry (read-only)
pub struct LogEntry {
    pub ts: String,
    pub source: String,
    pub level: String,
    pub detail: String,
}

/// Unified event picker / log viewer with source tabs
pub struct EventPicker {
    pub mode: EventPickerMode,
    // Source tabs
    pub sources: Vec<String>,
    pub active_tab: usize, // 0 = "All"
    // Picker mode (pending events with checkboxes)
    pub entries: Vec<EventEntry>,
    pub cursor: usize,
    // Log mode (history, read-only)
    pub log_entries: Vec<LogEntry>,
    pub log_scroll: u16,
}

impl EventPicker {
    /// Create a picker-mode view from buffered pending events (source, level, detail).
    /// All events start selected.
    pub fn new_picker(events: Vec<(String, String, String)>) -> Self {
        let mut source_set = Vec::new();
        let entries: Vec<EventEntry> = events
            .into_iter()
            .map(|(source, level, detail)| {
                if !source_set.contains(&source) {
                    source_set.push(source.clone());
                }
                EventEntry {
                    source,
                    level,
                    detail,
                    selected: true,
                }
            })
            .collect();
        let mut sources = vec!["All".to_string()];
        sources.extend(source_set);
        Self {
            mode: EventPickerMode::Picker,
            sources,
            active_tab: 0,
            entries,
            cursor: 0,
            log_entries: Vec::new(),
            log_scroll: 0,
        }
    }

    /// Create a log-mode view from historical events (ts, level, source, detail).
    pub fn new_log(events: Vec<(String, String, String, String)>) -> Self {
        let mut source_set = Vec::new();
        let log_entries: Vec<LogEntry> = events
            .into_iter()
            .map(|(ts, level, source, detail)| {
                if !source_set.contains(&source) {
                    source_set.push(source.clone());
                }
                LogEntry {
                    ts,
                    source,
                    level,
                    detail,
                }
            })
            .collect();
        let mut sources = vec!["All".to_string()];
        sources.extend(source_set);
        Self {
            mode: EventPickerMode::Log,
            sources,
            active_tab: 0,
            entries: Vec::new(),
            cursor: 0,
            log_entries,
            log_scroll: 0,
        }
    }

    /// Get the active source filter (None = "All")
    fn active_source(&self) -> Option<&str> {
        if self.active_tab == 0 {
            None
        } else {
            self.sources.get(self.active_tab).map(|s| s.as_str())
        }
    }

    /// Indices of picker entries matching the active tab filter
    pub fn filtered_entry_indices(&self) -> Vec<usize> {
        let filter = self.active_source();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| match filter {
                None => true,
                Some(src) => e.source == src,
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Log entries matching the active tab filter
    pub fn filtered_log_entries(&self) -> Vec<&LogEntry> {
        let filter = self.active_source();
        self.log_entries
            .iter()
            .filter(|e| match filter {
                None => true,
                Some(src) => e.source == src,
            })
            .collect()
    }

    /// Toggle the selected state of the entry at the cursor (picker mode)
    pub fn toggle_current(&mut self) {
        let indices = self.filtered_entry_indices();
        if let Some(&real_idx) = indices.get(self.cursor)
            && let Some(entry) = self.entries.get_mut(real_idx)
        {
            entry.selected = !entry.selected;
        }
    }

    /// Select all visible (filtered) entries
    pub fn select_all(&mut self) {
        let indices = self.filtered_entry_indices();
        for &i in &indices {
            self.entries[i].selected = true;
        }
    }

    /// Deselect all visible (filtered) entries
    pub fn deselect_all(&mut self) {
        let indices = self.filtered_entry_indices();
        for &i in &indices {
            self.entries[i].selected = false;
        }
    }

    /// Take the selected entries, returning (source, level, detail) tuples.
    /// Removes them from the picker; unselected entries remain.
    #[allow(clippy::type_complexity)]
    pub fn take_selected(
        &mut self,
    ) -> (Vec<(String, String, String)>, Vec<(String, String, String)>) {
        let mut selected = Vec::new();
        let mut remaining = Vec::new();
        for entry in self.entries.drain(..) {
            if entry.selected {
                selected.push((entry.source, entry.level, entry.detail));
            } else {
                remaining.push((entry.source, entry.level, entry.detail));
            }
        }
        (selected, remaining)
    }

    /// Return all entries back as (source, level, detail) tuples (e.g. on Esc)
    pub fn return_all(&mut self) -> Vec<(String, String, String)> {
        self.entries
            .drain(..)
            .map(|e| (e.source, e.level, e.detail))
            .collect()
    }

    /// Move to the next source tab (wrapping)
    pub fn next_tab(&mut self) {
        if !self.sources.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.sources.len();
            self.cursor = 0;
            self.log_scroll = 0;
        }
    }

    /// Move to the previous source tab (wrapping)
    pub fn prev_tab(&mut self) {
        if !self.sources.is_empty() {
            if self.active_tab == 0 {
                self.active_tab = self.sources.len() - 1;
            } else {
                self.active_tab -= 1;
            }
            self.cursor = 0;
            self.log_scroll = 0;
        }
    }
}

/// Widget that renders the EventPicker overlay
pub struct EventPickerWidget<'a> {
    picker: &'a EventPicker,
}

impl<'a> EventPickerWidget<'a> {
    pub fn new(picker: &'a EventPicker) -> Self {
        Self { picker }
    }
}

fn level_color(level: &str) -> Color {
    match level {
        "critical" => Color::Red,
        "error" => Color::LightRed,
        "warning" | "warn" => Color::Yellow,
        "info" => Color::Cyan,
        _ => Color::DarkGray,
    }
}

impl Widget for EventPickerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let mut lines: Vec<Line<'_>> = Vec::new();

        // Row 1: Header
        match self.picker.mode {
            EventPickerMode::Picker => {
                lines.push(Line::from(vec![
                    Span::styled(
                        " Event Picker",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "  ←→ tabs · ↑↓ nav · Space toggle · a=all · Enter send · Esc close",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
            EventPickerMode::Log => {
                lines.push(Line::from(vec![
                    Span::styled(
                        " Event Log",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "  ←→ tabs · ↑↓ scroll · Esc close",
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
            }
        }

        // Row 2: Source tabs
        let mut tab_spans: Vec<Span<'_>> = vec![Span::raw(" ")];
        for (i, src) in self.picker.sources.iter().enumerate() {
            if i == self.picker.active_tab {
                tab_spans.push(Span::styled(
                    format!("[{src}]"),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                tab_spans.push(Span::styled(
                    format!(" {src} "),
                    Style::default().fg(Color::DarkGray),
                ));
            }
            tab_spans.push(Span::raw(" "));
        }
        lines.push(Line::from(tab_spans));

        // Row 3: Separator
        let sep = "─".repeat(area.width.saturating_sub(2) as usize);
        lines.push(Line::from(Span::styled(
            format!(" {sep}"),
            Style::default().fg(Color::DarkGray),
        )));

        // Rows 4+: Content
        match self.picker.mode {
            EventPickerMode::Picker => {
                self.render_picker_entries(&mut lines, area);
            }
            EventPickerMode::Log => {
                self.render_log_entries(&mut lines, area);
            }
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}

impl EventPickerWidget<'_> {
    fn render_picker_entries(&self, lines: &mut Vec<Line<'_>>, area: Rect) {
        let indices = self.picker.filtered_entry_indices();

        if indices.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No pending events.",
                Style::default().fg(Color::DarkGray),
            )));
            return;
        }

        let available_rows = area.height.saturating_sub(5) as usize; // header + tabs + sep + footer
        let visible_items = available_rows.max(1);
        let total = indices.len();
        let cursor = self.picker.cursor.min(total.saturating_sub(1));
        let selected_count = indices
            .iter()
            .filter(|&&i| self.picker.entries[i].selected)
            .count();

        let scroll_offset = if cursor >= visible_items {
            cursor - visible_items + 1
        } else {
            0
        };
        let end = (scroll_offset + visible_items).min(total);

        for (vi, &real_idx) in indices.iter().enumerate().take(end).skip(scroll_offset) {
            let entry = &self.picker.entries[real_idx];
            let is_cursor = vi == cursor;

            let pointer = if is_cursor { "▸ " } else { "  " };
            let checkbox = if entry.selected { "[x]" } else { "[ ]" };
            let checkbox_color = if entry.selected {
                Color::Green
            } else {
                Color::Red
            };

            let lvl_color = level_color(&entry.level);

            let max_detail = (area.width as usize).saturating_sub(40);
            let detail_display = if entry.detail.len() > max_detail {
                format!("{}…", &entry.detail[..max_detail.saturating_sub(1)])
            } else {
                entry.detail.clone()
            };

            let pointer_style = if is_cursor {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };

            let detail_style = if is_cursor {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            lines.push(Line::from(vec![
                Span::styled(pointer, pointer_style),
                Span::styled(format!("{checkbox} "), Style::default().fg(checkbox_color)),
                Span::styled(
                    format!("[{}] ", entry.level.to_uppercase()),
                    Style::default().fg(lvl_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}: ", entry.source),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(detail_display, detail_style),
            ]));
        }

        // Footer
        if total > visible_items {
            lines.push(Line::from(Span::styled(
                format!("  ({}/{total})  {selected_count} selected", cursor + 1),
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                format!("  {selected_count}/{total} selected"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    fn render_log_entries(&self, lines: &mut Vec<Line<'_>>, area: Rect) {
        let filtered = self.picker.filtered_log_entries();

        if filtered.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No events recorded.",
                Style::default().fg(Color::DarkGray),
            )));
            return;
        }

        let available_rows = area.height.saturating_sub(5) as usize;
        let visible_items = available_rows.max(1);
        let total = filtered.len();
        let scroll = self.picker.log_scroll as usize;
        let start = scroll.min(total.saturating_sub(1));
        let end = (start + visible_items).min(total);

        for entry in &filtered[start..end] {
            let short_ts = if entry.ts.len() >= 19 {
                &entry.ts[11..19]
            } else {
                entry.ts.as_str()
            };
            let upper_level = entry.level.to_uppercase();
            let lvl_color = level_color(&entry.level);

            let max_detail = (area.width as usize).saturating_sub(35);
            let detail_display = if entry.detail.len() > max_detail {
                format!("{}…", &entry.detail[..max_detail.saturating_sub(1)])
            } else {
                entry.detail.clone()
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {short_ts} "),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("[{upper_level}] "),
                    Style::default().fg(lvl_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{}: ", entry.source),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(detail_display, Style::default().fg(Color::White)),
            ]));
        }

        // Footer
        lines.push(Line::from(Span::styled(
            format!("  ({total} events)"),
            Style::default().fg(Color::DarkGray),
        )));
    }
}
