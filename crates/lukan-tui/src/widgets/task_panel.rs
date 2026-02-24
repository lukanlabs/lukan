use lukan_tools::tasks::{TaskEntry, TaskStatus};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

/// Widget that renders the toggleable task panel above the input area.
pub struct TaskPanelWidget<'a> {
    entries: &'a [TaskEntry],
}

impl<'a> TaskPanelWidget<'a> {
    pub fn new(entries: &'a [TaskEntry]) -> Self {
        Self { entries }
    }
}

impl Widget for TaskPanelWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Tasks (Alt+T) ")
            .border_style(Style::default().fg(Color::DarkGray));

        let inner = block.inner(area);
        block.render(area, buf);

        if self.entries.is_empty() {
            let line = Line::from(Span::styled(
                "No active tasks.",
                Style::default().fg(Color::DarkGray),
            ));
            Paragraph::new(vec![line]).render(inner, buf);
            return;
        }

        let lines: Vec<Line<'_>> = self
            .entries
            .iter()
            .map(|entry| {
                let (icon, color) = match entry.status {
                    TaskStatus::Pending => ("⏳", Color::Yellow),
                    TaskStatus::InProgress => ("🔄", Color::Cyan),
                    TaskStatus::Done => ("✅", Color::Green),
                };
                Line::from(vec![
                    Span::raw(format!("{icon} ")),
                    Span::styled(
                        format!("#{}", entry.id),
                        Style::default()
                            .fg(color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" [{}]", entry.status.label()),
                        Style::default().fg(color),
                    ),
                    Span::styled(
                        format!(" — {}", entry.title),
                        Style::default().fg(Color::White),
                    ),
                ])
            })
            .collect();

        Paragraph::new(lines).render(inner, buf);
    }
}
