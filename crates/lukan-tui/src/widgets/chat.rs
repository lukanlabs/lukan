use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget, Wrap},
};

use syntect::easy::HighlightLines;

use super::markdown::{render_markdown, syntax_set, theme};
use super::shimmer::shimmer_spans;

/// Sanitize text for terminal display.
///
/// Replaces tab characters with spaces (terminals expand `\t` to variable-width
/// tab stops, but ratatui treats them as 0-width, causing ghost text artifacts).
/// Also strips ANSI escape sequences and other control characters that cause
/// width mismatches between ratatui's buffer and actual terminal rendering.
pub fn sanitize_for_display(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\t' {
            // Replace tab with 4 spaces (consistent width)
            out.push_str("    ");
            i += 1;
        } else if b == 0x1b {
            // Skip ANSI escape sequence: ESC [ ... final_byte
            i += 1;
            if i < bytes.len() && bytes[i] == b'[' {
                i += 1;
                // Consume until we hit a letter (0x40-0x7E)
                while i < bytes.len() && !(0x40..=0x7E).contains(&bytes[i]) {
                    i += 1;
                }
                if i < bytes.len() {
                    i += 1; // skip final byte
                }
            }
        } else if b == b'\n' {
            // Preserve newlines
            out.push('\n');
            i += 1;
        } else if b < 0x20 && b != b'\r' {
            // Skip other control characters (except CR which we just ignore)
            i += 1;
        } else {
            // Regular character — could be multi-byte UTF-8, use char_indices approach
            // For efficiency, find the next special byte and copy the chunk
            let start = i;
            i += 1;
            while i < bytes.len()
                && bytes[i] != b'\t'
                && bytes[i] != 0x1b
                && bytes[i] != b'\n'
                && (bytes[i] >= 0x20 || bytes[i] == b'\r' || bytes[i] >= 0x80)
            {
                i += 1;
            }
            out.push_str(&s[start..i]);
        }
    }
    out
}

/// A chat message for display
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// Optional unified diff for file changes (WriteFile/EditFile)
    pub diff: Option<String>,
    /// Tool use ID — pairs tool_call messages with their tool_result messages
    pub tool_id: Option<String>,
}

impl ChatMessage {
    /// Create a simple message without diff
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: sanitize_for_display(&content.into()),
            diff: None,
            tool_id: None,
        }
    }

    /// Create a message with an attached diff
    pub fn with_diff(
        role: impl Into<String>,
        content: impl Into<String>,
        diff: Option<String>,
    ) -> Self {
        Self {
            role: role.into(),
            content: sanitize_for_display(&content.into()),
            diff: diff.map(|d| sanitize_for_display(&d)),
            tool_id: None,
        }
    }
}

/// Build styled lines from a slice of messages and optional streaming text.
///
/// This is a standalone function used both by `ChatWidget::render` and by
/// `commit_overflow` in `app.rs` (to push old messages into the terminal
/// scrollback via `insert_before`).
///
/// `thinking_text`: accumulated reasoning/thinking from the model (Codex).
/// `is_streaming`: whether the model is currently generating output.
/// When both `thinking_text` is non-empty and `streaming_text` is empty
/// (model is in thinking phase), a shimmer "Thinking..." label is shown.
pub fn build_message_lines(
    messages: &[ChatMessage],
    streaming_text: &str,
    thinking_text: &str,
    is_streaming: bool,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "banner" => {
                for line in msg.content.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::White),
                    )));
                }
                lines.push(Line::from(""));
            }
            "tool_call" => {
                // Parse: ● ToolName(args) → ● cyan bold name + gray args
                let content = &msg.content;
                if let Some(rest) = content.strip_prefix("● ") {
                    if let Some(paren_pos) = rest.find('(') {
                        let tool_name = &rest[..paren_pos];
                        let args = &rest[paren_pos..];
                        lines.push(Line::from(vec![
                            Span::styled("  ● ", Style::default().fg(Color::Cyan)),
                            Span::styled(
                                tool_name.to_string(),
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(args.to_string(), Style::default().fg(Color::DarkGray)),
                        ]));
                    } else {
                        lines.push(Line::from(vec![
                            Span::styled("  ● ", Style::default().fg(Color::Cyan)),
                            Span::styled(
                                rest.to_string(),
                                Style::default()
                                    .fg(Color::Cyan)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    }
                } else {
                    // Fallback for non-standard format
                    lines.push(Line::from(Span::styled(
                        content.to_string(),
                        Style::default().fg(Color::White),
                    )));
                }
            }
            "tool_result" => {
                if msg.diff.is_some() {
                    // File change: show brief summary line, then parsed diff
                    let first_line = msg.content.lines().next().unwrap_or("  ⎿  (done)");
                    lines.push(Line::from(Span::styled(
                        first_line.to_string(),
                        Style::default().fg(Color::DarkGray),
                    )));
                    if let Some(ref diff) = msg.diff {
                        lines.extend(render_diff_lines(diff, 25));
                    }
                } else {
                    // No diff — show content truncated to keep output clean
                    let content_lines: Vec<&str> = msg.content.lines().collect();
                    let max_lines = 8;
                    let show = content_lines.len().min(max_lines);

                    for line in &content_lines[..show] {
                        let style = if line.contains("✗") {
                            Style::default().fg(Color::Red)
                        } else {
                            Style::default().fg(Color::DarkGray)
                        };
                        lines.push(Line::from(Span::styled(line.to_string(), style)));
                    }

                    if content_lines.len() > max_lines {
                        lines.push(Line::from(Span::styled(
                            format!("     ... ({} more lines)", content_lines.len() - max_lines),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                }
            }
            "system" => {
                for line in msg.content.lines() {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        Style::default().fg(Color::DarkGray),
                    )));
                }
                lines.push(Line::from(""));
            }
            "user" => {
                let bg_style = Style::default().fg(Color::White).bg(Color::DarkGray);
                for line in msg.content.lines() {
                    lines.push(
                        Line::from(Span::styled(format!("> {line}"), bg_style)).style(bg_style),
                    );
                }
                lines.push(Line::from(""));
            }
            _ => {
                // assistant — render markdown with styles & syntax highlighting
                lines.extend(render_markdown(&msg.content));
                lines.push(Line::from(""));
            }
        }
    }

    // Shimmer "Thinking..." indicator — shown during thinking phase
    // (model is streaming, thinking text is accumulating, but no text output yet)
    if is_streaming && !thinking_text.is_empty() && streaming_text.is_empty() {
        lines.push(Line::from(shimmer_spans("Thinking...")));
    }

    // Streaming text — render markdown with styles & syntax highlighting
    if !streaming_text.is_empty() {
        let sanitized = sanitize_for_display(streaming_text);
        lines.extend(render_markdown(&sanitized));
    }

    lines
}

/// Count physical rows that `lines` would occupy when wrapped at `width`.
/// This must match what `Paragraph::new(lines).wrap(Wrap { trim: false })`
/// actually renders.
pub fn physical_row_count(lines: &[Line], width: u16) -> u16 {
    let w = width.max(1) as usize;
    lines
        .iter()
        .map(|l| l.width().max(1).div_ceil(w))
        .sum::<usize>() as u16
}

/// Widget that renders the chat history
pub struct ChatWidget<'a> {
    messages: &'a [ChatMessage],
    streaming_text: &'a str,
    thinking_text: &'a str,
    is_streaming: bool,
}

impl<'a> ChatWidget<'a> {
    pub fn new(
        messages: &'a [ChatMessage],
        streaming_text: &'a str,
        thinking_text: &'a str,
        is_streaming: bool,
    ) -> Self {
        Self {
            messages,
            streaming_text,
            thinking_text,
            is_streaming,
        }
    }
}

impl Widget for ChatWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Clear the entire area first — ratatui double-buffers and Paragraph
        // only writes cells where it has text. Without this, characters from
        // previous frames bleed through as ghost text.
        Clear.render(area, buf);

        let lines = build_message_lines(
            self.messages,
            self.streaming_text,
            self.thinking_text,
            self.is_streaming,
        );

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });

        paragraph.render(area, buf);
    }
}

// ── Diff Rendering ─────────────────────────────────────────────────────

/// Parse and render a unified diff with syntax highlighting.
///
/// The diff string may begin with `--- path/to/file.ext` (produced by
/// EditFile/WriteFile) to enable language-aware syntax highlighting.
/// Token colours are mapped to ANSI-16 so they are always visible
/// regardless of the terminal's background colour.
///
/// Added lines get a dark-green background, removed lines dark-red.
/// Context lines have no background but still receive syntax colours.
fn render_diff_lines(diff: &str, max_changes: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut changes_shown: usize = 0;
    let mut consecutive_blank_ctx: usize = 0;

    // ── Syntax highlighting setup ────────────────────────────────────
    let ss = syntax_set();
    let ext = diff
        .lines()
        .find(|l| l.starts_with("--- "))
        .and_then(|l| l.strip_prefix("--- "))
        .and_then(|path| path.rsplit('.').next());
    let syntax = ext.and_then(|e| {
        ss.find_syntax_by_extension(e)
            .or_else(|| ss.find_syntax_by_token(e))
    });
    // Separate highlighter per stream so syntect parser state stays valid.
    let mut hl_add = syntax.map(|s| HighlightLines::new(s, theme()));
    let mut hl_del = syntax.map(|s| HighlightLines::new(s, theme()));
    let mut hl_ctx = syntax.map(|s| HighlightLines::new(s, theme()));

    // ── Background tints ─────────────────────────────────────────────
    let add_bg = Color::Rgb(20, 40, 20);
    let del_bg = Color::Rgb(50, 20, 20);

    // ── Change count (for truncation message) ────────────────────────
    let total_changes = diff
        .lines()
        .filter(|l| {
            (l.starts_with('+') && !l.starts_with("+++"))
                || (l.starts_with('-') && !l.starts_with("---"))
        })
        .count();

    for raw_line in diff.lines() {
        // Skip diff metadata headers
        if raw_line.starts_with("diff --git")
            || raw_line.starts_with("index ")
            || raw_line.starts_with("--- ")
            || raw_line.starts_with("+++")
            || raw_line.starts_with("new file")
            || raw_line.starts_with("deleted file")
            || raw_line.starts_with("similarity")
            || raw_line.starts_with("rename")
        {
            continue;
        }

        // Hunk header → dim cyan separator
        if raw_line.starts_with("@@") {
            consecutive_blank_ctx = 0;
            lines.push(Line::from(Span::styled(
                format!("     {raw_line}"),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::DIM),
            )));
            continue;
        }

        let is_add = raw_line.starts_with('+');
        let is_remove = raw_line.starts_with('-');

        if is_add || is_remove {
            consecutive_blank_ctx = 0;
            changes_shown += 1;
            if changes_shown > max_changes {
                continue;
            }
        } else if changes_shown > max_changes {
            continue;
        }

        if is_add {
            let code = &raw_line[1..];
            let line = diff_line(code, &mut hl_add, ss, "     +", Color::LightGreen, Some(add_bg));
            lines.push(line);
        } else if is_remove {
            let code = &raw_line[1..];
            let line = diff_line(code, &mut hl_del, ss, "     -", Color::LightRed, Some(del_bg));
            lines.push(line);
        } else {
            // Context line — collapse consecutive blank lines to max 1
            let content = raw_line.strip_prefix(' ').unwrap_or(raw_line);
            if content.trim().is_empty() {
                consecutive_blank_ctx += 1;
                if consecutive_blank_ctx > 1 {
                    continue;
                }
            } else {
                consecutive_blank_ctx = 0;
            }
            let line = diff_line(content, &mut hl_ctx, ss, "      ", Color::DarkGray, None);
            lines.push(line);
        }
    }

    if total_changes > max_changes {
        lines.push(Line::from(Span::styled(
            format!(
                "     ... ({} more changes not shown)",
                total_changes - max_changes
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines
}

/// Map a syntect RGB colour to an ANSI-16 colour by analysing the hue.
///
/// ANSI-16 colours are defined by the terminal's colour scheme, so they
/// are always legible regardless of the terminal background — unlike the
/// muted pastels that syntect themes produce for dark backgrounds.
fn rgb_to_ansi16(r: u8, g: u8, b: u8) -> Color {
    let max = r.max(g).max(b) as i16;
    let min = r.min(g).min(b) as i16;
    let chroma = max - min;

    // Very dark → comments / preprocessor directives
    if max < 90 {
        return Color::DarkGray;
    }
    // Near-grey (low saturation) → default text
    if chroma < 30 {
        return Color::Reset; // terminal default fg
    }

    let (r, g, b) = (r as i16, g as i16, b as i16);

    // Dominant hue → ANSI colour
    if r >= g && r >= b {
        if g >= b {
            Color::LightYellow // orange / yellow  (numbers, constants)
        } else {
            Color::LightMagenta // pink / magenta   (keywords)
        }
    } else if g >= r && g >= b {
        if r >= b {
            Color::LightYellow // yellow-green       (operators)
        } else {
            Color::LightGreen // green               (strings)
        }
    } else {
        // blue is dominant
        if g >= r {
            Color::LightCyan // cyan                 (builtins / types)
        } else {
            Color::LightBlue // blue / purple        (variables / names)
        }
    }
}

/// Render one diff line with syntax highlighting.
///
/// `prefix`       — the `"     +"` / `"     -"` / `"      "` leader.
/// `prefix_color` — colour for the prefix glyph.
/// `bg`           — optional background tint for the whole line.
fn diff_line(
    code: &str,
    highlighter: &mut Option<HighlightLines<'_>>,
    ss: &syntect::parsing::SyntaxSet,
    prefix: &str,
    prefix_color: Color,
    bg: Option<Color>,
) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Prefix glyph  (e.g. "     +" in bright green)
    let prefix_style = match bg {
        Some(b) => Style::default().fg(prefix_color).bg(b),
        None => Style::default().fg(prefix_color),
    };
    spans.push(Span::styled(prefix.to_string(), prefix_style));

    // Syntax-highlighted tokens
    // `highlight_line` expects a newline-terminated string for correct
    // multi-line parser state; we append one here.
    let line_with_nl = format!("{code}\n");
    let ranges = highlighter
        .as_mut()
        .and_then(|h| h.highlight_line(&line_with_nl, ss).ok());

    match ranges {
        Some(tokens) => {
            for (style, text) in tokens {
                let text = text.trim_end_matches('\n');
                if text.is_empty() {
                    continue;
                }
                let fg = rgb_to_ansi16(style.foreground.r, style.foreground.g, style.foreground.b);
                let s = match bg {
                    Some(b) => Style::default().fg(fg).bg(b),
                    None => Style::default().fg(fg),
                };
                spans.push(Span::styled(text.to_string(), s));
            }
        }
        None => {
            // No syntax info — plain coloured text
            let s = match bg {
                Some(b) => Style::default().fg(Color::White).bg(b),
                None => Style::default().fg(Color::White),
            };
            spans.push(Span::styled(code.to_string(), s));
        }
    }

    // Line-level bg fills the remainder of the terminal width
    let line_style = match bg {
        Some(b) => Style::default().bg(b),
        None => Style::default(),
    };
    Line::from(spans).style(line_style)
}
