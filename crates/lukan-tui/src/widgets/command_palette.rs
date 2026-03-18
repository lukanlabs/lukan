use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget},
};

pub(crate) struct CommandPaletteWidget<'a> {
    commands: &'a [(&'static str, &'static str)],
    selected: usize,
}

impl<'a> CommandPaletteWidget<'a> {
    pub(crate) fn new(commands: &'a [(&'static str, &'static str)], selected: usize) -> Self {
        Self { commands, selected }
    }
}

impl Widget for CommandPaletteWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let mut lines: Vec<Line<'_>> = Vec::new();

        // Blank separator line at top
        lines.push(Line::from(""));

        for (i, (cmd, desc)) in self.commands.iter().enumerate() {
            let is_selected = i == self.selected;
            let pointer = if is_selected { "▸ " } else { "  " };

            // Pad command name to align descriptions
            let padded_cmd = format!("{cmd:<14}");

            if is_selected {
                lines.push(Line::from(vec![
                    Span::styled(
                        pointer,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        padded_cmd,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled((*desc).to_string(), Style::default().fg(Color::White)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(Color::DarkGray)),
                    Span::styled(padded_cmd, Style::default().fg(Color::Gray)),
                    Span::styled((*desc).to_string(), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}
