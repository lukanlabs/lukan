use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

/// Status bar showing provider, model, and token usage
pub struct StatusBarWidget<'a> {
    provider: &'a str,
    model: &'a str,
    input_tokens: u64,
    output_tokens: u64,
    is_streaming: bool,
}

impl<'a> StatusBarWidget<'a> {
    pub fn new(
        provider: &'a str,
        model: &'a str,
        input_tokens: u64,
        output_tokens: u64,
        is_streaming: bool,
    ) -> Self {
        Self {
            provider,
            model,
            input_tokens,
            output_tokens,
            is_streaming,
        }
    }
}

impl Widget for StatusBarWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let status_indicator = if self.is_streaming {
            Span::styled(
                " ● streaming ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(" ● ready ", Style::default().fg(Color::Cyan))
        };

        let provider_info = Span::styled(
            format!(" {} / {} ", self.provider, self.model),
            Style::default().fg(Color::DarkGray),
        );

        let tokens = Span::styled(
            format!(" tokens: {}↓ {}↑ ", self.input_tokens, self.output_tokens),
            Style::default().fg(Color::DarkGray),
        );

        let quit_hint = Span::styled(" Ctrl+C quit ", Style::default().fg(Color::DarkGray));

        let line = Line::from(vec![status_indicator, provider_info, tokens, quit_hint]);

        let paragraph = Paragraph::new(line);
        paragraph.render(area, buf);
    }
}
