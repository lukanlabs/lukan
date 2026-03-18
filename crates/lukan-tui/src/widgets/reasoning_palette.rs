use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget},
};

use crate::app::ReasoningPicker;

pub(crate) struct ReasoningPaletteWidget<'a> {
    picker: &'a ReasoningPicker,
}

impl<'a> ReasoningPaletteWidget<'a> {
    pub(crate) fn new(picker: &'a ReasoningPicker) -> Self {
        Self { picker }
    }
}

impl Widget for ReasoningPaletteWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let mut lines: Vec<Line<'_>> = Vec::new();

        // Header with model name
        let model_name = self
            .picker
            .model_entry
            .split_once(':')
            .map(|(_, m)| m)
            .unwrap_or(&self.picker.model_entry);
        lines.push(Line::from(Span::styled(
            format!("  Select Reasoning Level for {model_name}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        for (i, (_value, label, desc)) in self.picker.levels.iter().enumerate() {
            let is_selected = i == self.picker.selected;
            let pointer = if is_selected { "▸ " } else { "  " };
            let num = format!("{}. ", i + 1);
            let padded_label = format!("{label:<20}");

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
                        padded_label,
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled((*desc).to_string(), Style::default().fg(Color::Gray)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(pointer, Style::default().fg(Color::DarkGray)),
                    Span::styled(num, Style::default().fg(Color::DarkGray)),
                    Span::styled(padded_label, Style::default().fg(Color::Gray)),
                    Span::styled((*desc).to_string(), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, buf);
    }
}
