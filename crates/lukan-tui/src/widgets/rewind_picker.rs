use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget},
};

/// View mode for the rewind picker
#[derive(Clone, Copy, PartialEq)]
pub enum RewindView {
    /// List of checkpoints
    List,
    /// Restore options (Chat only / Chat + Code)
    Options,
}

/// A single entry in the rewind picker
pub struct RewindEntry {
    /// Checkpoint ID (None for the "(current)" sentinel)
    pub checkpoint_id: Option<String>,
    /// User message that triggered this checkpoint
    pub message: String,
    /// Number of files changed
    pub files_changed: usize,
    /// Total line additions
    pub additions: u32,
    /// Total line deletions
    pub deletions: u32,
}

/// State for the rewind picker overlay
pub struct RewindPicker {
    pub entries: Vec<RewindEntry>,
    pub selected: usize,
    pub view: RewindView,
    /// 0 = Chat only, 1 = Chat + Code
    pub option_idx: usize,
}

impl RewindPicker {
    pub fn new(entries: Vec<RewindEntry>) -> Self {
        // Default selection to last entry (= "(current)")
        let selected = entries.len().saturating_sub(1);
        Self {
            entries,
            selected,
            view: RewindView::List,
            option_idx: 0,
        }
    }

    /// The selected entry's checkpoint_id (None if "(current)")
    pub fn selected_checkpoint_id(&self) -> Option<&str> {
        self.entries
            .get(self.selected)
            .and_then(|e| e.checkpoint_id.as_deref())
    }
}

/// Stateless widget that renders a `RewindPicker`
pub struct RewindPickerWidget<'a> {
    picker: &'a RewindPicker,
}

impl<'a> RewindPickerWidget<'a> {
    pub fn new(picker: &'a RewindPicker) -> Self {
        Self { picker }
    }
}

impl Widget for RewindPickerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        match self.picker.view {
            RewindView::List => render_list(self.picker, area, buf),
            RewindView::Options => render_options(self.picker, area, buf),
        }
    }
}

fn render_list(picker: &RewindPicker, area: Rect, buf: &mut Buffer) {
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Header
    let count = picker
        .entries
        .iter()
        .filter(|e| e.checkpoint_id.is_some())
        .count();
    let title = format!(" Rewind ({count}) ");
    let hints = "↑↓ navigate · Enter restore · ESC close";

    // Build header line: title left, hints right
    let title_len = title.len();
    let hints_len = hints.len();
    let padding = (area.width as usize)
        .saturating_sub(title_len + hints_len)
        .max(1);

    lines.push(Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(padding)),
        Span::styled(hints, Style::default().fg(Color::DarkGray)),
    ]));
    lines.push(Line::from(""));

    // Scrolling viewport
    let available_rows = area.height.saturating_sub(2) as usize; // header + blank
    let items_per_entry = 2usize; // message line + stats line
    let visible_entries = available_rows / items_per_entry;

    // Calculate scroll offset to keep selected visible
    let scroll_offset = if visible_entries == 0 {
        0
    } else if picker.selected >= visible_entries {
        picker.selected - visible_entries + 1
    } else {
        0
    };

    let max_msg_width = (area.width as usize).saturating_sub(6); // "❯  " + padding

    for (i, entry) in picker
        .entries
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_entries.max(1))
    {
        let is_selected = i == picker.selected;
        let pointer = if is_selected { "❯ " } else { "  " };

        // First line: pointer + message (truncated)
        let display_msg = if entry.checkpoint_id.is_none() {
            "(current)".to_string()
        } else {
            let msg = entry.message.lines().next().unwrap_or(&entry.message);
            if msg.len() > max_msg_width {
                format!("{}…", &msg[..max_msg_width.saturating_sub(1)])
            } else {
                msg.to_string()
            }
        };

        if is_selected {
            lines.push(Line::from(vec![
                Span::styled(
                    pointer,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    display_msg,
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(pointer, Style::default().fg(Color::DarkGray)),
                Span::styled(display_msg, Style::default().fg(Color::Gray)),
            ]));
        }

        // Second line: stats
        let stats = if entry.checkpoint_id.is_none() {
            String::new()
        } else if entry.files_changed == 0 {
            "   No code changes".to_string()
        } else {
            let files_word = if entry.files_changed == 1 {
                "file"
            } else {
                "files"
            };
            format!(
                "   {} {} changed +{} -{}",
                entry.files_changed, files_word, entry.additions, entry.deletions
            )
        };

        lines.push(Line::from(Span::styled(
            stats,
            Style::default().fg(Color::DarkGray),
        )));
    }

    let paragraph = Paragraph::new(lines);
    paragraph.render(area, buf);
}

fn render_options(picker: &RewindPicker, area: Rect, buf: &mut Buffer) {
    let mut lines: Vec<Line<'_>> = Vec::new();

    // Show which checkpoint we're restoring to
    let msg_preview = picker
        .entries
        .get(picker.selected)
        .map(|e| {
            let msg = e.message.lines().next().unwrap_or(&e.message);
            if msg.len() > 50 {
                let end = msg.floor_char_boundary(49);
                format!("{}…", &msg[..end])
            } else {
                msg.to_string()
            }
        })
        .unwrap_or_default();

    lines.push(Line::from(vec![
        Span::styled(
            " Restore to: ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(msg_preview, Style::default().fg(Color::White)),
    ]));
    lines.push(Line::from(""));

    let options = [
        ("Chat only", "Rewind conversation history only"),
        ("Chat + Code", "Rewind conversation and revert file changes"),
    ];

    for (i, (label, desc)) in options.iter().enumerate() {
        let is_selected = i == picker.option_idx;
        let pointer = if is_selected { "❯ " } else { "  " };

        if is_selected {
            lines.push(Line::from(vec![
                Span::styled(
                    pointer,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{label:<16}"),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(*desc, Style::default().fg(Color::Gray)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(pointer, Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{label:<16}"), Style::default().fg(Color::Gray)),
                Span::styled(*desc, Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ↑↓ navigate · Enter confirm · ESC back",
        Style::default().fg(Color::DarkGray),
    )));

    let paragraph = Paragraph::new(lines);
    paragraph.render(area, buf);
}
