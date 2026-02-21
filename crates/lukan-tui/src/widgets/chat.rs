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
            // Role header
            let (role_text, role_style) = match msg.role.as_str() {
                "user" => (
                    "You",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                "assistant" => (
                    "lukan",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                _ => ("system", Style::default().fg(Color::Yellow)),
            };

            lines.push(Line::from(Span::styled(role_text, role_style)));

            // Content lines
            for line in msg.content.lines() {
                lines.push(Line::from(line.to_string()));
            }

            lines.push(Line::from("")); // Spacing between messages
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
