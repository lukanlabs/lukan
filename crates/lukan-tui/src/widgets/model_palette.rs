use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget},
};

use crate::app::ModelPicker;

pub(crate) struct ModelPaletteWidget<'a> {
    picker: &'a ModelPicker,
}

impl<'a> ModelPaletteWidget<'a> {
    pub(crate) fn new(picker: &'a ModelPicker) -> Self {
        Self { picker }
    }
}

impl Widget for ModelPaletteWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let mut lines: Vec<Line<'_>> = Vec::new();

        // Header
        lines.push(Line::from(vec![
            Span::styled(
                "  Select Model",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  (d = set as default)",
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        lines.push(Line::from(""));

        for (i, entry) in self.picker.models.iter().enumerate() {
            let is_selected = i == self.picker.selected;
            let is_current = *entry == self.picker.current;

            let pointer = if is_selected { "▸ " } else { "  " };
            let num = format!("{}. ", i + 1);

            let suffix = if is_current { " (current)" } else { "" };

            if is_selected {
                lines.push(Line::from(vec![
                    Span::styled(
                        pointer,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        num,
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        entry.clone(),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(suffix, Style::default().fg(Color::Green)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(Color::DarkGray)),
                    Span::styled(num, Style::default().fg(Color::DarkGray)),
                    Span::styled(entry.clone(), Style::default().fg(Color::Gray)),
                    Span::styled(suffix, Style::default().fg(Color::Green)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}
