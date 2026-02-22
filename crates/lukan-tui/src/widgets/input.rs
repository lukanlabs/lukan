use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use unicode_width::UnicodeWidthStr;

/// Text input widget with cursor and line wrapping
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

/// Calculate how many rows the input text needs (including border).
/// Returns at least 3 (1 line + 2 border rows), capped at `max_lines` content lines.
pub fn input_height(text: &str, area_width: u16, max_lines: u16) -> u16 {
    let inner = area_width.saturating_sub(2).max(1) as usize;
    if text.is_empty() {
        return 3;
    }
    let display_w = UnicodeWidthStr::width(text);
    let lines = display_w.div_ceil(inner);
    let lines = (lines as u16).clamp(1, max_lines);
    lines + 2
}

/// Calculate the (x, y) cursor position inside the input area, accounting for wrapping.
/// `text` is the full input, `byte_pos` is the byte offset of the cursor.
/// Returns (cursor_x, cursor_y) in absolute terminal coordinates.
pub fn cursor_position(text: &str, byte_pos: usize, area: Rect) -> (u16, u16) {
    let inner_w = area.width.saturating_sub(2).max(1) as usize;
    // Display width of text before cursor
    let before = &text[..byte_pos.min(text.len())];
    let col_total = UnicodeWidthStr::width(before);
    let row = col_total / inner_w;
    let col = col_total % inner_w;
    (area.x + 1 + col as u16, area.y + 1 + row as u16)
}

impl Widget for InputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let is_shell = self.text.starts_with('!');

        let border_color = if is_shell {
            Color::Red
        } else if self.is_focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let title = if is_shell { " $ " } else { " > " };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(title);

        let display_text = if self.text.is_empty() {
            Line::from(Span::styled(
                "Type a message...",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Line::from(self.text)
        };

        let paragraph = Paragraph::new(display_text)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
