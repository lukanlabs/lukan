use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::app::TrustPrompt;

pub(crate) struct TrustPromptWidget<'a> {
    prompt: &'a TrustPrompt,
}

impl<'a> TrustPromptWidget<'a> {
    pub(crate) fn new(prompt: &'a TrustPrompt) -> Self {
        Self { prompt }
    }
}

impl Widget for TrustPromptWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let yes_pointer = if self.prompt.selected == 0 {
            "❯ "
        } else {
            "  "
        };
        let no_pointer = if self.prompt.selected == 1 {
            "❯ "
        } else {
            "  "
        };

        let yes_style = if self.prompt.selected == 0 {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let no_style = if self.prompt.selected == 1 {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                " Workspace access:",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                format!(" {}", self.prompt.cwd),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " Quick safety check: Is this a project you created or",
                Style::default().fg(Color::White),
            )),
            Line::from(Span::styled(
                " one you trust? If you're not sure, take a moment to",
                Style::default().fg(Color::White),
            )),
            Line::from(Span::styled(
                " review what's in this folder first.",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " lukan will be able to read, edit, and execute code",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                " and files in this directory.",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled(format!(" {yes_pointer}"), yes_style),
                Span::styled("1. Yes, I trust this folder", yes_style),
            ]),
            Line::from(vec![
                Span::styled(format!(" {no_pointer}"), no_style),
                Span::styled("2. No, exit", no_style),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                " Enter to confirm · Esc to cancel",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
