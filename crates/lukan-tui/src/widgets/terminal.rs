use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Widget},
};

use crate::terminal_modal::{CellColor, Screen, Selection};

/// Renders a terminal Screen into a ratatui buffer as a bordered overlay.
pub struct TerminalWidget<'a> {
    screen: &'a Screen,
    exited: bool,
    selection: Option<&'a Selection>,
}

impl<'a> TerminalWidget<'a> {
    pub fn new(screen: &'a Screen, exited: bool) -> Self {
        Self {
            screen,
            exited,
            selection: None,
        }
    }

    pub fn with_selection(mut self, selection: Option<&'a Selection>) -> Self {
        self.selection = selection;
        self
    }
}

impl Widget for TerminalWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let screen = self.screen;
        let scrolled = screen.scroll_offset > 0;

        let border_color = if self.exited {
            Color::DarkGray
        } else {
            Color::Cyan
        };

        let title = if self.exited {
            " Terminal (exited) — F9 close ".to_string()
        } else if scrolled {
            format!(
                " Terminal [-{}/{}] Ctrl+\u{2191}\u{2193} scroll | F9 min ",
                screen.scroll_offset,
                screen.scrollback_len()
            )
        } else {
            " Terminal — F9 min | Ctrl+Shift+F9 close | Ctrl+\u{2191}\u{2193} scroll ".to_string()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(Color::Black));
        let inner = block.inner(area);
        block.render(area, buf);

        let (screen_rows, screen_cols) = screen.size();

        for row in 0..inner.height.min(screen_rows) {
            for col in 0..inner.width.min(screen_cols) {
                let Some(cell) = screen.visible_cell(row, col) else {
                    continue;
                };

                if cell.wide_continuation {
                    continue;
                }

                let x = inner.x + col;
                let y = inner.y + row;
                if x >= inner.x + inner.width || y >= inner.y + inner.height {
                    continue;
                }

                let ch = cell.ch;
                let symbol: String = if ch == ' ' || ch == '\0' {
                    " ".into()
                } else {
                    ch.to_string()
                };

                let fg = convert_color(cell.fg);
                let bg = convert_color(cell.bg);

                let mut style = Style::default();

                if cell.inverse {
                    style = style.fg(bg).bg(fg);
                } else {
                    style = style.fg(fg).bg(bg);
                }

                if cell.bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if cell.italic {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                if cell.underline {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                if cell.dim {
                    style = style.add_modifier(Modifier::DIM);
                }

                // Highlight selected cells
                if let Some(sel) = self.selection
                    && sel.contains(row, col)
                {
                    // Resolve Reset to concrete colors before swapping
                    let actual_fg = match style.fg {
                        Some(Color::Reset) | None => Color::White,
                        Some(c) => c,
                    };
                    let actual_bg = match style.bg {
                        Some(Color::Reset) | None => Color::Black,
                        Some(c) => c,
                    };
                    style = style.fg(actual_bg).bg(actual_fg);
                }

                let buf_cell = &mut buf[(x, y)];
                buf_cell.set_symbol(&symbol);
                buf_cell.set_style(style);
            }
        }

        // Render cursor only when at live view (not scrolled back)
        if !self.exited && !scrolled && screen.cursor_visible {
            let (crow, ccol) = screen.cursor_position();
            if crow < inner.height && ccol < inner.width {
                let cx = inner.x + ccol;
                let cy = inner.y + crow;
                if let Some(existing) = buf.cell_mut((cx, cy)) {
                    // Solid inverted block cursor — always visible
                    existing.set_style(
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    );
                }
            }
        }
    }
}

/// Map our CellColor to ratatui Color.
fn convert_color(color: CellColor) -> Color {
    match color {
        CellColor::Default => Color::Reset,
        CellColor::Idx(0) => Color::Black,
        CellColor::Idx(1) => Color::Red,
        CellColor::Idx(2) => Color::Green,
        CellColor::Idx(3) => Color::Yellow,
        CellColor::Idx(4) => Color::Blue,
        CellColor::Idx(5) => Color::Magenta,
        CellColor::Idx(6) => Color::Cyan,
        CellColor::Idx(7) => Color::Gray,
        CellColor::Idx(8) => Color::DarkGray,
        CellColor::Idx(9) => Color::LightRed,
        CellColor::Idx(10) => Color::LightGreen,
        CellColor::Idx(11) => Color::LightYellow,
        CellColor::Idx(12) => Color::LightBlue,
        CellColor::Idx(13) => Color::LightMagenta,
        CellColor::Idx(14) => Color::LightCyan,
        CellColor::Idx(15) => Color::White,
        CellColor::Idx(idx) => Color::Indexed(idx),
        CellColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}
