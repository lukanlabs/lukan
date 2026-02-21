use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

/// Text input widget with cursor
pub struct InputWidget<'a> {
    text: &'a str,
    cursor_pos: usize,
    is_focused: bool,
}

impl<'a> InputWidget<'a> {
    pub fn new(text: &'a str, cursor_pos: usize, is_focused: bool) -> Self {
        Self {
            text,
            cursor_pos,
            is_focused,
        }
    }
}

impl Widget for InputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.is_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" > ");

        let display_text = if self.text.is_empty() {
            Line::from(Span::styled(
                "Type a message...",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(self.text)
        };

        let paragraph = Paragraph::new(display_text).block(block);
        paragraph.render(area, buf);
    }
}
