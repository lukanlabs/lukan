use std::sync::OnceLock;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme, ThemeSet},
    parsing::SyntaxSet,
};

// ── Lazy-initialized syntax highlighting resources ──────────────────────

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME: OnceLock<Theme> = OnceLock::new();

pub(crate) fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

pub(crate) fn theme() -> &'static Theme {
    THEME.get_or_init(|| {
        let ts = ThemeSet::load_defaults();
        ts.themes["base16-ocean.dark"].clone()
    })
}

// ── Public API ──────────────────────────────────────────────────────────

/// Parse a markdown string and return styled ratatui `Line`s.
///
/// Handles headings, bold/italic/strikethrough, inline code, fenced code
/// blocks with syntax highlighting (via syntect), blockquotes, ordered and
/// unordered lists, task list markers, links, and horizontal rules.
pub fn render_markdown(input: &str) -> Vec<Line<'static>> {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(input, opts);

    let mut renderer = MdRenderer::new();
    for event in parser {
        renderer.process_event(event);
    }
    renderer.finish()
}

// ── Internal types ──────────────────────────────────────────────────────

/// Tracks indentation context for nested block elements.
struct IndentCtx {
    /// Continuation prefix (spaces matching marker width).
    prefix: String,
    /// First-line marker (e.g. `"• "`, `"1. "`). Cleared after first flush.
    marker: Option<String>,
    /// Whether this level is a blockquote (affects prefix styling).
    is_blockquote: bool,
}

/// Stateful markdown-to-ratatui converter.
struct MdRenderer {
    lines: Vec<Line<'static>>,
    style_stack: Vec<Style>,
    indent_stack: Vec<IndentCtx>,
    list_indices: Vec<Option<u64>>,
    current_spans: Vec<Span<'static>>,
    in_code_block: bool,
    code_block_lang: String,
    code_block_buf: String,
    link_url: Option<String>,
    // Table support: buffer cells, then render on TagEnd::Table
    in_table: bool,
    table_rows: Vec<Vec<String>>,
    table_current_row: Vec<String>,
    table_cell_buf: String,
    table_is_header: bool,
}

impl MdRenderer {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            style_stack: Vec::new(),
            indent_stack: Vec::new(),
            list_indices: Vec::new(),
            current_spans: Vec::new(),
            in_code_block: false,
            code_block_lang: String::new(),
            code_block_buf: String::new(),
            link_url: None,
            in_table: false,
            table_rows: Vec::new(),
            table_current_row: Vec::new(),
            table_cell_buf: String::new(),
            table_is_header: false,
        }
    }

    // ── Style helpers ───────────────────────────────────────────────────

    /// Merge all active inline styles into one.
    fn current_style(&self) -> Style {
        self.style_stack
            .iter()
            .fold(Style::default(), |acc, s| acc.patch(*s))
    }

    /// How many list levels deep we are (0-based).
    fn list_depth(&self) -> usize {
        self.list_indices.len().saturating_sub(1)
    }

    // ── Event dispatch ──────────────────────────────────────────────────

    fn process_event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.push_text(&text),
            Event::Code(code) => self.push_inline_code(&code),
            Event::SoftBreak => self.soft_break(),
            Event::HardBreak => self.hard_break(),
            Event::Rule => self.horizontal_rule(),
            Event::TaskListMarker(checked) => self.task_marker(checked),
            _ => {}
        }
    }

    // ── Tag start ───────────────────────────────────────────────────────

    fn start_tag(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush_line();
                let style = match level {
                    HeadingLevel::H1 => Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    HeadingLevel::H2 => Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    HeadingLevel::H3 => {
                        Style::default().add_modifier(Modifier::BOLD | Modifier::ITALIC)
                    }
                    _ => Style::default().add_modifier(Modifier::ITALIC),
                };
                self.style_stack.push(style);
            }
            Tag::Strong => {
                self.style_stack
                    .push(Style::default().add_modifier(Modifier::BOLD));
            }
            Tag::Emphasis => {
                self.style_stack
                    .push(Style::default().add_modifier(Modifier::ITALIC));
            }
            Tag::Strikethrough => {
                self.style_stack
                    .push(Style::default().add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::BlockQuote(_) => {
                self.flush_line();
                self.style_stack.push(Style::default().fg(Color::Green));
                self.indent_stack.push(IndentCtx {
                    prefix: "│ ".to_string(),
                    marker: None,
                    is_blockquote: true,
                });
            }
            Tag::List(start) => {
                self.list_indices.push(start);
            }
            Tag::Item => {
                self.flush_line();
                let depth = self.list_depth();
                let indent = "  ".repeat(depth);
                let marker = match self.list_indices.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{indent}{n}. ");
                        *n += 1;
                        m
                    }
                    _ => format!("{indent}• "),
                };
                let continuation = " ".repeat(marker.len());
                self.indent_stack.push(IndentCtx {
                    prefix: continuation,
                    marker: Some(marker),
                    is_blockquote: false,
                });
            }
            Tag::CodeBlock(kind) => {
                self.flush_line();
                self.in_code_block = true;
                self.code_block_lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code_block_buf.clear();
            }
            Tag::Table(_alignments) => {
                self.flush_line();
                self.in_table = true;
                self.table_rows.clear();
            }
            Tag::TableHead => {
                self.table_is_header = true;
                self.table_current_row.clear();
            }
            Tag::TableRow => {
                self.table_is_header = false;
                self.table_current_row.clear();
            }
            Tag::TableCell => {
                self.table_cell_buf.clear();
            }
            Tag::Link { dest_url, .. } => {
                self.link_url = Some(dest_url.to_string());
                self.style_stack.push(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                );
            }
            Tag::Paragraph => { /* handled by End */ }
            _ => {}
        }
    }

    // ── Tag end ─────────────────────────────────────────────────────────

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.style_stack.pop();
                self.flush_line();
                self.lines.push(Line::from(""));
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => {
                self.style_stack.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_line();
                self.style_stack.pop();
                self.indent_stack.pop();
            }
            TagEnd::List(_) => {
                self.list_indices.pop();
            }
            TagEnd::Item => {
                self.flush_line();
                self.indent_stack.pop();
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                let code = self.code_block_buf.trim_end_matches('\n');
                let highlighted = highlight_code(code, &self.code_block_lang);
                self.lines.extend(highlighted);
                self.lines.push(Line::from(""));
                self.code_block_buf.clear();
                self.code_block_lang.clear();
            }
            TagEnd::Table => {
                self.in_table = false;
                self.render_table();
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                let row = std::mem::take(&mut self.table_current_row);
                self.table_rows.push(row);
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.table_cell_buf);
                self.table_current_row.push(cell.trim().to_string());
            }
            TagEnd::Link => {
                if let Some(url) = self.link_url.take() {
                    self.current_spans.push(Span::styled(
                        format!(" ({url})"),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                self.style_stack.pop();
            }
            TagEnd::Paragraph => {
                self.flush_line();
                // Add blank line after paragraph, but not inside list items
                // (tight lists don't emit Paragraph events; loose lists do).
                let in_list = self.indent_stack.iter().any(|c| !c.is_blockquote);
                if !in_list {
                    self.lines.push(Line::from(""));
                }
            }
            _ => {}
        }
    }

    // ── Inline content ──────────────────────────────────────────────────

    fn push_text(&mut self, text: &str) {
        if self.in_code_block {
            self.code_block_buf.push_str(text);
            return;
        }
        if self.in_table {
            self.table_cell_buf.push_str(text);
            return;
        }
        let style = self.current_style();
        for (i, segment) in text.split('\n').enumerate() {
            if i > 0 {
                self.flush_line();
            }
            if !segment.is_empty() {
                self.current_spans
                    .push(Span::styled(segment.to_string(), style));
            }
        }
    }

    fn push_inline_code(&mut self, code: &str) {
        if self.in_table {
            self.table_cell_buf.push_str(code);
            return;
        }
        self.current_spans.push(Span::styled(
            code.to_string(),
            Style::default().fg(Color::Cyan),
        ));
    }

    fn soft_break(&mut self) {
        if self.in_table {
            self.table_cell_buf.push(' ');
            return;
        }
        let style = self.current_style();
        self.current_spans
            .push(Span::styled(" ".to_string(), style));
    }

    fn hard_break(&mut self) {
        if self.in_table {
            self.table_cell_buf.push(' ');
            return;
        }
        self.flush_line();
    }

    fn horizontal_rule(&mut self) {
        self.flush_line();
        self.lines.push(Line::from(Span::styled(
            "─".repeat(40),
            Style::default().fg(Color::DarkGray),
        )));
        self.lines.push(Line::from(""));
    }

    fn task_marker(&mut self, checked: bool) {
        let marker = if checked { "☑ " } else { "☐ " };
        // Replace the bullet marker with a task marker
        if let Some(ctx) = self.indent_stack.last_mut()
            && let Some(ref mut m) = ctx.marker
            && let Some(pos) = m.rfind("• ")
        {
            let prefix = &m[..pos];
            *m = format!("{prefix}{marker}");
        }
    }

    // ── Table rendering ──────────────────────────────────────────────────

    /// Render buffered table rows with aligned columns.
    fn render_table(&mut self) {
        if self.table_rows.is_empty() {
            return;
        }

        // Calculate column widths
        let num_cols = self.table_rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut col_widths = vec![0usize; num_cols];
        for row in &self.table_rows {
            for (i, cell) in row.iter().enumerate() {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }

        // Render each row
        for (row_idx, row) in self.table_rows.iter().enumerate() {
            let mut spans: Vec<Span<'static>> = Vec::new();
            spans.push(Span::styled("  ", Style::default()));

            for (i, cell) in row.iter().enumerate() {
                let width = col_widths.get(i).copied().unwrap_or(0);
                let padded = format!("{:<width$}", cell, width = width);

                let style = if row_idx == 0 {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                if i > 0 {
                    spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
                }
                spans.push(Span::styled(padded, style));
            }

            self.lines.push(Line::from(spans));

            // Add separator after header row
            if row_idx == 0 {
                let sep: String = col_widths
                    .iter()
                    .map(|w| "─".repeat(*w))
                    .collect::<Vec<_>>()
                    .join("─┼─");
                self.lines.push(Line::from(Span::styled(
                    format!("  {sep}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        self.lines.push(Line::from(""));
        self.table_rows.clear();
    }

    // ── Line management ─────────────────────────────────────────────────

    /// Flush accumulated spans into a finished `Line`, prepending any indent.
    fn flush_line(&mut self) {
        if self.current_spans.is_empty() {
            return;
        }

        let mut spans = Vec::new();

        // Build indent prefix from the stack
        let indent = self.build_indent_prefix();
        if !indent.is_empty() {
            let indent_style = if self.indent_stack.iter().any(|c| c.is_blockquote) {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };
            spans.push(Span::styled(indent, indent_style));
        }

        spans.extend(std::mem::take(&mut self.current_spans));
        self.lines.push(Line::from(spans));

        // After the first line of an item, clear the marker for continuation
        if let Some(ctx) = self.indent_stack.last_mut()
            && ctx.marker.is_some()
        {
            ctx.marker = None;
        }
    }

    /// Concatenate all active indent prefixes / markers.
    fn build_indent_prefix(&self) -> String {
        let mut prefix = String::new();
        for ctx in &self.indent_stack {
            if let Some(ref marker) = ctx.marker {
                prefix.push_str(marker);
            } else {
                prefix.push_str(&ctx.prefix);
            }
        }
        prefix
    }

    /// Consume the renderer and return the accumulated lines.
    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_line();
        // Remove trailing empty lines
        while self
            .lines
            .last()
            .is_some_and(|l| l.spans.is_empty() || l.width() == 0)
        {
            self.lines.pop();
        }
        self.lines
    }
}

// ── Syntax highlighting ─────────────────────────────────────────────────

/// Highlight a code block using syntect, returning indented ratatui lines.
/// Falls back to plain cyan if the language is unrecognised.
fn highlight_code(code: &str, lang: &str) -> Vec<Line<'static>> {
    let ss = syntax_set();
    let syntax = if lang.is_empty() {
        None
    } else {
        ss.find_syntax_by_token(lang)
    };

    match syntax {
        Some(syntax) => {
            let mut h = HighlightLines::new(syntax, theme());
            let mut lines = Vec::new();
            for line in code.lines() {
                match h.highlight_line(line, ss) {
                    Ok(ranges) => {
                        let mut spans: Vec<Span<'static>> = vec![Span::raw("    ")];
                        for (style, text) in ranges {
                            spans.push(Span::styled(text.to_string(), syntect_to_ratatui(style)));
                        }
                        lines.push(Line::from(spans));
                    }
                    Err(_) => {
                        lines.push(Line::from(vec![
                            Span::raw("    "),
                            Span::styled(line.to_string(), Style::default().fg(Color::Cyan)),
                        ]));
                    }
                }
            }
            lines
        }
        None => {
            // Unknown language — plain cyan
            code.lines()
                .map(|line| {
                    Line::from(vec![
                        Span::raw("    "),
                        Span::styled(line.to_string(), Style::default().fg(Color::Cyan)),
                    ])
                })
                .collect()
        }
    }
}

/// Convert a syntect `Style` to a ratatui `Style`.
fn syntect_to_ratatui(style: syntect::highlighting::Style) -> Style {
    let fg = style.foreground;
    let mut s = Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b));
    if style.font_style.contains(FontStyle::BOLD) {
        s = s.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_renders_unchanged() {
        let lines = render_markdown("hello world");
        assert_eq!(lines.len(), 1);
        assert!(lines[0].to_string().contains("hello world"));
    }

    #[test]
    fn heading_produces_styled_line() {
        let lines = render_markdown("# Title");
        assert_eq!(lines.len(), 1); // heading line (trailing blank stripped)
        let line = &lines[0];
        assert!(line.to_string().contains("Title"));
    }

    #[test]
    fn unordered_list() {
        let lines = render_markdown("- one\n- two\n- three");
        assert!(lines.len() >= 3);
        assert!(lines[0].to_string().contains('•'));
        assert!(lines[0].to_string().contains("one"));
    }

    #[test]
    fn ordered_list() {
        let lines = render_markdown("1. first\n2. second");
        assert!(lines.len() >= 2);
        assert!(lines[0].to_string().contains("1."));
        assert!(lines[0].to_string().contains("first"));
    }

    #[test]
    fn code_block_indented() {
        let lines = render_markdown("```\nfoo\nbar\n```");
        // Each code line should start with 4-space indent
        for line in &lines {
            let text = line.to_string();
            if !text.is_empty() {
                assert!(text.starts_with("    "), "expected indent: {:?}", text);
            }
        }
    }

    #[test]
    fn inline_code_cyan() {
        let lines = render_markdown("use `foo` here");
        assert_eq!(lines.len(), 1);
        // Should contain the inline code text
        let text = lines[0].to_string();
        assert!(text.contains("foo"));
    }

    #[test]
    fn horizontal_rule() {
        let lines = render_markdown("above\n\n---\n\nbelow");
        let rule_line = lines.iter().find(|l| l.to_string().contains('─'));
        assert!(rule_line.is_some());
    }

    #[test]
    fn blockquote_prefixed() {
        let lines = render_markdown("> quoted text");
        assert!(lines.len() >= 1);
        let text = lines[0].to_string();
        assert!(text.contains('│'));
        assert!(text.contains("quoted text"));
    }

    #[test]
    fn syntax_highlighted_code() {
        let md = "```rust\nfn main() {}\n```";
        let lines = render_markdown(md);
        // Should produce at least one line with indented code
        assert!(!lines.is_empty());
        let text = lines[0].to_string();
        assert!(text.contains("fn") || text.contains("main"));
    }

    #[test]
    fn empty_input() {
        let lines = render_markdown("");
        assert!(lines.is_empty());
    }
}
