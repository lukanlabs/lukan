use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::app::{PlanReviewMode, PlanReviewState};
use crate::widgets::markdown::render_markdown;

pub(crate) struct PlanReviewWidget<'a> {
    state: &'a PlanReviewState,
}

impl<'a> PlanReviewWidget<'a> {
    pub(crate) fn new(state: &'a PlanReviewState) -> Self {
        Self { state }
    }
}

impl Widget for PlanReviewWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let mut lines: Vec<Line<'_>> = Vec::new();

        match self.state.mode {
            PlanReviewMode::List => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!(" Plan: {}", self.state.title),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));

                for (i, task) in self.state.tasks.iter().enumerate() {
                    let is_selected = i == self.state.selected;
                    let pointer = if is_selected { "▸ " } else { "  " };
                    let style = if is_selected {
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Gray)
                    };

                    lines.push(Line::from(vec![
                        Span::styled(format!(" {pointer}"), style),
                        Span::styled(format!("{}. {}", i + 1, task.title), style),
                    ]));
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " a=accept · r=request changes · Enter=view detail · Esc=reject",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            PlanReviewMode::Detail => {
                let task = &self.state.tasks[self.state.selected];
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!(" Task {}: {}", self.state.selected + 1, task.title),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));

                for line in render_markdown(&task.detail) {
                    lines.push(line);
                }

                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " Esc=back to list  ↑↓/jk=scroll",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            PlanReviewMode::Feedback => {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " Request Changes",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " Type your feedback:",
                    Style::default().fg(Color::White),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    format!(" > {}_", self.state.feedback_input),
                    Style::default().fg(Color::Green),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " Enter=submit · Esc=cancel",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Plan Review ")
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.state.scroll, 0));
        paragraph.render(area, buf);
    }
}
