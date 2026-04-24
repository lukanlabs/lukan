use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};

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
    let mut rows: usize = 1;
    let mut col: usize = 0;
    for ch in text.chars() {
        if ch == '\n' {
            rows += 1;
            col = 0;
        } else {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if col + w > inner {
                rows += 1;
                col = w;
            } else {
                col += w;
                if col == inner {
                    rows += 1;
                    col = 0;
                }
            }
        }
    }
    let lines = (rows as u16).clamp(1, max_lines);
    lines + 2
}

/// Calculate the (x, y) cursor position inside the input area, accounting for wrapping and newlines.
/// `text` is the full input, `byte_pos` is the byte offset of the cursor.
/// Returns (cursor_x, cursor_y) in absolute terminal coordinates.
pub fn cursor_position(text: &str, byte_pos: usize, area: Rect) -> (u16, u16) {
    let inner_w = area.width.saturating_sub(2).max(1) as usize;
    let before = &text[..byte_pos.min(text.len())];
    let mut row: usize = 0;
    let mut col: usize = 0;

    for ch in before.chars() {
        if ch == '\n' {
            row += 1;
            col = 0;
        } else {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if col + w > inner_w {
                row += 1;
                col = w;
            } else {
                col += w;
                if col == inner_w {
                    row += 1;
                    col = 0;
                }
            }
        }
    }

    (area.x + 1 + col as u16, area.y + 1 + row as u16)
}

impl Widget for InputWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let is_shell = self.text.starts_with('!');

        let border_color = if is_shell {
            Color::Red
        } else if self.is_focused {
            Color::Rgb(170, 170, 170)
        } else {
            Color::DarkGray
        };

        let title = if is_shell { " $ " } else { " > " };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(title);

        let inner_w = area.width.saturating_sub(2).max(1) as usize;
        let display_text: Vec<Line> = if self.text.is_empty() {
            vec![Line::from(Span::styled(
                "Type a message...",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            // Manual char-by-char wrap to match cursor_position exactly
            let mut lines: Vec<Line> = Vec::new();
            let mut current = String::new();
            let mut col: usize = 0;
            for ch in self.text.chars() {
                if ch == '\n' {
                    lines.push(Line::from(std::mem::take(&mut current)));
                    col = 0;
                } else {
                    let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if col + w > inner_w {
                        lines.push(Line::from(std::mem::take(&mut current)));
                        current.push(ch);
                        col = w;
                    } else {
                        current.push(ch);
                        col += w;
                        if col == inner_w {
                            lines.push(Line::from(std::mem::take(&mut current)));
                            col = 0;
                        }
                    }
                }
            }
            lines.push(Line::from(current));
            lines
        };

        let paragraph = Paragraph::new(display_text).block(block);
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_first_line() {
        let area = Rect::new(0, 0, 42, 5); // width 42, inner = 40
        // Cursor at end of "hello"
        let (x, y) = cursor_position("hello", 5, area);
        assert_eq!((x, y), (6, 1)); // x=0+1+5, y=0+1
    }

    #[test]
    fn test_cursor_after_newline() {
        let area = Rect::new(0, 0, 42, 5);
        // "hello\n" - cursor at start of second line
        let (x, y) = cursor_position("hello\n", 6, area);
        assert_eq!((x, y), (1, 2)); // x=0+1+0, y=0+1+1
    }

    #[test]
    fn test_cursor_second_line_text() {
        let area = Rect::new(0, 0, 42, 5);
        // "hello\nwor" - cursor at end of "wor"
        let (x, y) = cursor_position("hello\nwor", 9, area);
        assert_eq!((x, y), (4, 2)); // x=0+1+3, y=0+1+1
    }

    #[test]
    fn test_cursor_wrap() {
        let area = Rect::new(0, 0, 12, 5); // inner width = 10
        // 15 chars wraps to 2 visual lines
        let (x, y) = cursor_position("abcdefghijklmno", 15, area);
        assert_eq!((x, y), (6, 2)); // 10 on row 1, 5 on row 2: x=0+1+5, y=0+1+1
    }

    #[test]
    fn test_cursor_newline_then_type() {
        let area = Rect::new(0, 0, 42, 5);
        // "hello\nworld" - cursor at 'd'
        let (x, y) = cursor_position("hello\nworld", 11, area);
        assert_eq!((x, y), (6, 2)); // x=0+1+5, y=0+1+1
    }
}
