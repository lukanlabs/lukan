use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::app::ToolPicker;

pub(crate) struct ToolPickerWidget<'a> {
    picker: &'a ToolPicker,
}

impl<'a> ToolPickerWidget<'a> {
    pub(crate) fn new(picker: &'a ToolPicker) -> Self {
        Self { picker }
    }
}

impl Widget for ToolPickerWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let mut lines: Vec<Line<'_>> = Vec::new();
        lines.push(Line::from(""));

        let mut tool_row = 0usize;
        let mut selected_line = 0usize;

        for group in &self.picker.groups {
            lines.push(Line::from(Span::styled(
                format!(" ── {} ──", group.name),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));

            for tool in &group.tools {
                let is_selected = tool_row == self.picker.selected;
                if is_selected {
                    selected_line = lines.len();
                }
                let is_disabled = self.picker.disabled.contains(tool);
                let pointer = if is_selected { "▸ " } else { "  " };
                let checkbox = if is_disabled { "[ ]" } else { "[x]" };
                let checkbox_style = if is_disabled {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                };

                lines.push(Line::from(vec![
                    Span::styled(pointer.to_string(), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{checkbox} "), checkbox_style),
                    Span::styled(
                        tool.clone(),
                        if is_selected {
                            Style::default()
                                .fg(Color::White)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            Style::default().fg(Color::Gray)
                        },
                    ),
                ]));

                tool_row += 1;
            }
            lines.push(Line::from(""));
        }

        let available_rows = area.height.saturating_sub(2) as usize;
        let scroll_y = if selected_line >= available_rows {
            (selected_line - available_rows + 1) as u16
        } else {
            0
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Tools (Alt+P) · Space toggle ")
            .border_style(Style::default().fg(Color::Cyan))
            .style(Style::default().bg(Color::Rgb(20, 20, 20)));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll_y, 0))
            .style(Style::default().bg(Color::Rgb(20, 20, 20)));
        paragraph.render(area, buf);
    }
}
