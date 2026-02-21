use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};

/// A chat message for display
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// Optional unified diff for file changes (WriteFile/EditFile)
    pub diff: Option<String>,
}

impl ChatMessage {
    /// Create a simple message without diff
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            diff: None,
        }
    }

    /// Create a message with an attached diff
    pub fn with_diff(
        role: impl Into<String>,
        content: impl Into<String>,
        diff: Option<String>,
    ) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            diff,
        }
    }
}

/// Widget that renders the chat history
pub struct ChatWidget<'a> {
    messages: &'a [ChatMessage],
    streaming_text: &'a str,
    scroll_offset: u16,
}

impl<'a> ChatWidget<'a> {
    pub fn new(messages: &'a [ChatMessage], streaming_text: &'a str, scroll_offset: u16) -> Self {
        Self {
            messages,
            streaming_text,
            scroll_offset,
        }
    }

    fn render_messages(&self) -> Vec<Line<'a>> {
        let mut lines = Vec::new();

        for msg in self.messages {
            match msg.role.as_str() {
                "banner" => {
                    // Banner lines are pre-styled, render as-is
                    for line in msg.content.lines() {
                        lines.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(Color::Cyan),
                        )));
                    }
                    lines.push(Line::from(""));
                }
                "tool_call" => {
                    // ● ToolName(input) — yellow bullet, white content
                    for line in msg.content.lines() {
                        if line.starts_with('●') {
                            lines.push(Line::from(vec![
                                Span::styled("● ", Style::default().fg(Color::Yellow)),
                                Span::styled(
                                    line.trim_start_matches('●').trim_start().to_string(),
                                    Style::default().fg(Color::White),
                                ),
                            ]));
                        } else {
                            lines.push(Line::from(Span::styled(
                                line.to_string(),
                                Style::default().fg(Color::White),
                            )));
                        }
                    }
                }
                "tool_result" => {
                    // ⎿ result summary — dim gray
                    for line in msg.content.lines() {
                        lines.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                    // Render diff if present (max 20 changed lines)
                    if let Some(ref diff) = msg.diff {
                        let max_lines = 20;
                        let mut shown = 0;
                        let total_changed = diff
                            .lines()
                            .filter(|l| l.starts_with('+') || l.starts_with('-'))
                            .count();

                        for diff_line in diff.lines() {
                            // Only count +/- lines toward the limit
                            let is_change =
                                diff_line.starts_with('+') || diff_line.starts_with('-');
                            if is_change {
                                shown += 1;
                                if shown > max_lines {
                                    continue;
                                }
                            } else if shown > max_lines {
                                continue;
                            }

                            let line_obj = if diff_line.starts_with('+') {
                                Line::from(Span::styled(
                                    format!("     {diff_line}"),
                                    Style::default().fg(Color::Green),
                                ))
                            } else if diff_line.starts_with('-') {
                                Line::from(Span::styled(
                                    format!("     {diff_line}"),
                                    Style::default().fg(Color::Red),
                                ))
                            } else {
                                Line::from(Span::styled(
                                    format!("     {diff_line}"),
                                    Style::default().fg(Color::DarkGray),
                                ))
                            };
                            lines.push(line_obj);
                        }

                        if total_changed > max_lines {
                            lines.push(Line::from(Span::styled(
                                format!(
                                    "     ... ({} more changes not shown)",
                                    total_changed - max_lines
                                ),
                                Style::default().fg(Color::DarkGray),
                            )));
                        }
                    }
                }
                "system" => {
                    for line in msg.content.lines() {
                        lines.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                    lines.push(Line::from(""));
                }
                "user" => {
                    lines.push(Line::from(Span::styled(
                        "You",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    )));
                    for line in msg.content.lines() {
                        lines.push(Line::from(line.to_string()));
                    }
                    lines.push(Line::from(""));
                }
                _ => {
                    // assistant
                    lines.push(Line::from(Span::styled(
                        "lukan",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )));
                    for line in msg.content.lines() {
                        lines.push(Line::from(line.to_string()));
                    }
                    lines.push(Line::from(""));
                }
            }
        }

        // Streaming text
        if !self.streaming_text.is_empty() {
            lines.push(Line::from(Span::styled(
                "lukan",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            for line in self.streaming_text.lines() {
                lines.push(Line::from(line.to_string()));
            }
        }

        lines
    }
}

impl Widget for ChatWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines = self.render_messages();

        let block = Block::default().borders(Borders::NONE);

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        paragraph.render(area, buf);
    }
}

/// Count the total number of rendered lines (for auto-scroll calculation)
pub fn rendered_line_count(messages: &[ChatMessage], streaming_text: &str) -> u16 {
    let tmp = ChatWidget::new(messages, streaming_text, 0);
    tmp.render_messages().len() as u16
}
