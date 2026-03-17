use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::app::PlannerQuestionState;

pub(crate) struct PlannerQuestionWidget<'a> {
    state: &'a PlannerQuestionState,
}

impl<'a> PlannerQuestionWidget<'a> {
    pub(crate) fn new(state: &'a PlannerQuestionState) -> Self {
        Self { state }
    }
}

impl Widget for PlannerQuestionWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let qi = self.state.current_question;
        let q = &self.state.questions[qi];

        let mut lines: Vec<Line<'_>> = Vec::new();

        lines.push(Line::from(""));

        // Tab headers
        let mut tab_spans: Vec<Span<'_>> = vec![Span::raw(" ")];
        for (i, question) in self.state.questions.iter().enumerate() {
            let style = if i == qi {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            tab_spans.push(Span::styled(format!(" {} ", question.header), style));
            if i + 1 < self.state.questions.len() {
                tab_spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
            }
        }
        lines.push(Line::from(tab_spans));
        lines.push(Line::from(""));

        // Question text
        lines.push(Line::from(Span::styled(
            format!(" {}", q.question),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        // Options
        let custom_idx = q.options.len(); // virtual "Custom response..." index
        for (i, opt) in q.options.iter().enumerate() {
            let is_selected = i == self.state.selections[qi];
            let is_checked = q.multi_select
                && self.state.multi_selections[qi]
                    .get(i)
                    .copied()
                    .unwrap_or(false);

            let pointer = if is_selected { "▸ " } else { "  " };
            let checkbox = if q.multi_select {
                if is_checked { "[x] " } else { "[ ] " }
            } else if is_selected {
                "(●) "
            } else {
                "( ) "
            };

            let style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            lines.push(Line::from(vec![
                Span::styled(format!(" {pointer}"), style),
                Span::styled(checkbox.to_string(), style),
                Span::styled(opt.label.clone(), style),
            ]));

            if let Some(ref desc) = opt.description {
                lines.push(Line::from(Span::styled(
                    format!("       {desc}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        // "Custom response..." option (always last)
        {
            let is_selected = self.state.selections[qi] == custom_idx;
            let pointer = if is_selected { "▸ " } else { "  " };
            let radio = if is_selected { "(●) " } else { "( ) " };
            let style = if is_selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            lines.push(Line::from(vec![
                Span::styled(format!(" {pointer}"), style),
                Span::styled(radio.to_string(), style),
                Span::styled("Custom response...", style),
            ]));
        }

        // Custom text input area (shown when editing_custom is active)
        if self.state.editing_custom && self.state.selections[qi] == custom_idx {
            lines.push(Line::from(""));
            let input_text = &self.state.custom_inputs[qi];
            let display = if input_text.is_empty() {
                vec![Span::styled(
                    "  Type your response here…",
                    Style::default().fg(Color::DarkGray),
                )]
            } else {
                vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(input_text.clone(), Style::default().fg(Color::White)),
                    Span::styled("█", Style::default().fg(Color::Yellow)),
                ]
            };
            lines.push(Line::from(display));
        }

        lines.push(Line::from(""));
        let hint = if self.state.editing_custom {
            " Type response · Enter submit · Esc back"
        } else if self.state.questions.len() > 1 {
            " ↑↓ select · Space/Enter choose · Tab next · Enter confirm · Esc cancel"
        } else {
            " ↑↓ select · Space/Enter choose · Enter confirm · Esc cancel"
        };
        lines.push(Line::from(Span::styled(
            hint,
            Style::default().fg(Color::DarkGray),
        )));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Planner Question ")
            .border_style(Style::default().fg(Color::Magenta));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
