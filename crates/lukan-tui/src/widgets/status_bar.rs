use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

/// Status bar showing provider, model, token usage, and permission mode
pub struct StatusBarWidget<'a> {
    provider: &'a str,
    model: &'a str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    context_size: u64,
    is_streaming: bool,
    active_tool: Option<&'a str>,
    memory_active: bool,
    permission_mode: &'a str,
    /// Optional view label (e.g. "Events") shown as a badge
    view_label: Option<&'a str>,
    /// Whether the Event Agent has unread messages (shown as badge in Main view)
    event_unread: bool,
}

impl<'a> StatusBarWidget<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: &'a str,
        model: &'a str,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_creation_tokens: u64,
        context_size: u64,
        is_streaming: bool,
        active_tool: Option<&'a str>,
        memory_active: bool,
        permission_mode: &'a str,
    ) -> Self {
        Self {
            provider,
            model,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            context_size,
            is_streaming,
            active_tool,
            memory_active,
            permission_mode,
            view_label: None,
            event_unread: false,
        }
    }

    pub fn view_label(mut self, label: Option<&'a str>) -> Self {
        self.view_label = label;
        self
    }

    pub fn event_unread(mut self, unread: bool) -> Self {
        self.event_unread = unread;
        self
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

        // Context size with compaction threshold indicator
        let ctx_color = if self.context_size >= 150_000 {
            Color::Red
        } else if self.context_size >= 100_000 {
            Color::Yellow
        } else {
            Color::DarkGray
        };
        let ctx_text = if self.context_size > 0 {
            format!(" ctx: {}k/150k", self.context_size / 1000)
        } else {
            String::new()
        };
        let context_span = Span::styled(ctx_text, Style::default().fg(ctx_color));

        // Cache info
        let cache_text = if self.cache_read_tokens > 0 || self.cache_creation_tokens > 0 {
            let mut parts = Vec::new();
            if self.cache_read_tokens > 0 {
                parts.push(format!("{}k hit", self.cache_read_tokens / 1000));
            }
            if self.cache_creation_tokens > 0 {
                parts.push(format!("{}k write", self.cache_creation_tokens / 1000));
            }
            format!(" cache: {}", parts.join(" "))
        } else {
            String::new()
        };
        let cache_span = Span::styled(cache_text, Style::default().fg(Color::DarkGray));

        let tokens = Span::styled(
            format!(" tokens: {}↓ {}↑", self.input_tokens, self.output_tokens),
            Style::default().fg(Color::DarkGray),
        );

        let mode_color = match self.permission_mode {
            "manual" => Color::Yellow,
            "auto" => Color::Green,
            "skip" => Color::Cyan,
            "planner" => Color::Magenta,
            _ => Color::DarkGray,
        };
        let mode_indicator = Span::styled(
            format!(" [{}] ", self.permission_mode),
            Style::default().fg(mode_color),
        );

        let memory_indicator = if self.memory_active {
            Span::styled(" [memory] ", Style::default().fg(Color::Magenta))
        } else {
            Span::raw("")
        };

        // Show Alt+B hint when Bash tool is actively running
        let bash_hint = if self.active_tool == Some("Bash") {
            Span::styled(" Alt+B background ", Style::default().fg(Color::Yellow))
        } else {
            Span::raw("")
        };

        let view_indicator = if let Some(label) = self.view_label {
            Span::styled(
                format!(" [{label}] "),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw("")
        };

        let unread_badge = if self.event_unread {
            Span::styled(
                " [Events!] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw("")
        };

        let quit_hint = Span::styled(" Ctrl+C quit ", Style::default().fg(Color::DarkGray));

        let line = Line::from(vec![
            status_indicator,
            view_indicator,
            provider_info,
            context_span,
            cache_span,
            tokens,
            mode_indicator,
            memory_indicator,
            unread_badge,
            bash_hint,
            quit_hint,
        ]);

        let paragraph = Paragraph::new(line);
        paragraph.render(area, buf);
    }
}
