use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget, Wrap},
};

use similar::{ChangeTag, TextDiff};
use syntect::easy::HighlightLines;

use super::markdown::{render_markdown, syntax_set, theme};


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
pub fn build_message_lines(
    messages: &[ChatMessage],
    streaming_text: &str,
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
                if let Some(ref diff) = msg.diff {
                    // File change: show stats line + diff
                    if let Some(stats_line) = msg.content.lines().find(|l| l.contains('⎿')) {
                        lines.push(Line::from(Span::styled(
                            stats_line.to_string(),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                    lines.extend(render_diff_lines(diff, 25));
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
}

impl<'a> ChatWidget<'a> {
    pub fn new(
        messages: &'a [ChatMessage],
        streaming_text: &'a str,
    ) -> Self {
        Self {
            messages,
            streaming_text,
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
        );

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });

        paragraph.render(area, buf);
    }
}

// ── Diff Rendering ─────────────────────────────────────────────────────

// Colour palette
const ADD_BG: Color = Color::Rgb(20, 60, 20); // vivid green background for added lines
const DEL_BG: Color = Color::Rgb(70, 20, 20); // vivid red background for removed lines
const ADD_HL: Color = Color::Rgb(60, 130, 60); // brighter highlight for the changed chars (add)
const DEL_HL: Color = Color::Rgb(130, 45, 45); // brighter highlight for the changed chars (del)

/// Parse `@@ -old_start[,count] +new_start[,count] @@` hunk headers.
/// Returns `(old_start, new_start)` as 1-based line numbers.
fn parse_hunk_header(line: &str) -> Option<(u32, u32)> {
    // Format: @@ -L[,N] +L[,N] @@
    let inner = line.strip_prefix("@@ ")?.split(" @@").next()?;
    let mut parts = inner.split_whitespace();
    let old_part = parts.next()?.strip_prefix('-')?;
    let new_part = parts.next()?.strip_prefix('+')?;
    let old_start: u32 = old_part.split(',').next()?.parse().ok()?;
    let new_start: u32 = new_part.split(',').next()?.parse().ok()?;
    Some((old_start, new_start))
}

/// Build the `"   NNN {sign}"` prefix for a diff line.
fn line_prefix(num: u32, sign: char, bg: Option<Color>) -> Span<'static> {
    let text = format!("  {:>4} {sign}", num);
    let style = match (sign, bg) {
        ('+', Some(b)) => Style::default().fg(Color::LightGreen).bg(b),
        ('-', Some(b)) => Style::default().fg(Color::LightRed).bg(b),
        (_, Some(b)) => Style::default().fg(Color::DarkGray).bg(b),
        ('+', None) => Style::default().fg(Color::LightGreen),
        ('-', None) => Style::default().fg(Color::LightRed),
        _ => Style::default().fg(Color::DarkGray),
    };
    Span::styled(text, style)
}

/// Build spans for a changed line with inline char-level diff highlighting.
///
/// `is_add`: true for the added version, false for the removed version.
/// Characters that were inserted (for add) or deleted (for del) get the
/// brighter highlight background so they visually pop.
fn inline_changed_spans(code: &str, counterpart: &str, bg: Color, hl: Color) -> Vec<Span<'static>> {
    let diff = TextDiff::from_chars(counterpart, code);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut buf = String::new();
    let mut in_highlight = false;

    for change in diff.iter_all_changes() {
        // `code` is the "new" in from_chars(counterpart, code),
        // so its chars are Insert (unique to code) + Equal (shared).
        let relevant = change.tag() == ChangeTag::Insert || change.tag() == ChangeTag::Equal;
        if !relevant {
            continue;
        }
        let is_changed = change.tag() != ChangeTag::Equal;

        if is_changed != in_highlight && !buf.is_empty() {
            let b = if in_highlight { hl } else { bg };
            spans.push(Span::styled(buf.clone(), Style::default().fg(Color::White).bg(b)));
            buf.clear();
        }
        in_highlight = is_changed;
        buf.push_str(change.value());
    }
    if !buf.is_empty() {
        let b = if in_highlight { hl } else { bg };
        spans.push(Span::styled(buf, Style::default().fg(Color::White).bg(b)));
    }
    spans
}

/// Flush buffered del/add blocks into rendered lines with optional inline diff.
///
/// When `del_buf` and `add_buf` have the same length, each pair of lines
/// gets character-level inline diff highlighting. Otherwise they are rendered
/// with solid colours.
fn flush_blocks(
    del_buf: &[(String, u32)],
    add_buf: &[(String, u32)],
    out: &mut Vec<Line<'static>>,
) {
    let paired = del_buf.len() == add_buf.len() && !del_buf.is_empty();

    // Emit del lines
    for (i, (code, num)) in del_buf.iter().enumerate() {
        let prefix = line_prefix(*num, '-', Some(DEL_BG));
        let mut spans = vec![prefix];
        if paired {
            spans.extend(inline_changed_spans(code, &add_buf[i].0, DEL_BG, DEL_HL));
        } else {
            spans.push(Span::styled(
                code.clone(),
                Style::default().fg(Color::White).bg(DEL_BG),
            ));
        }
        out.push(Line::from(spans).style(Style::default().bg(DEL_BG)));
    }

    // Emit add lines
    for (i, (code, num)) in add_buf.iter().enumerate() {
        let prefix = line_prefix(*num, '+', Some(ADD_BG));
        let mut spans = vec![prefix];
        if paired {
            spans.extend(inline_changed_spans(code, &del_buf[i].0, ADD_BG, ADD_HL));
        } else {
            spans.push(Span::styled(
                code.clone(),
                Style::default().fg(Color::White).bg(ADD_BG),
            ));
        }
        out.push(Line::from(spans).style(Style::default().bg(ADD_BG)));
    }
}

/// Parse and render a unified diff.
///
/// Features:
/// - Line numbers from hunk headers
/// - Vivid green/red backgrounds for add/del lines
/// - Inline char-level diff highlight for paired add/del lines
/// - Syntax colours (ANSI-16) for context lines
fn render_diff_lines(diff: &str, max_changes: usize) -> Vec<Line<'static>> {
    // ── Syntax highlighting for context lines ────────────────────────
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
    let mut hl_ctx = syntax.map(|s| HighlightLines::new(s, theme()));

    let total_changes = diff
        .lines()
        .filter(|l| {
            (l.starts_with('+') && !l.starts_with("+++"))
                || (l.starts_with('-') && !l.starts_with("---"))
        })
        .count();

    let mut out: Vec<Line<'static>> = Vec::new();
    let mut changes_shown: usize = 0;
    let mut consecutive_blank_ctx: usize = 0;
    let mut hunk_count: usize = 0;

    // Line number tracking
    let mut old_line: u32 = 1;
    let mut new_line: u32 = 1;

    // Buffers for del/add blocks (for inline diff pairing)
    let mut del_buf: Vec<(String, u32)> = Vec::new();
    let mut add_buf: Vec<(String, u32)> = Vec::new();

    for raw_line in diff.lines() {
        // Skip metadata headers
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

        // Hunk header — flush buffers, reset line counters
        if raw_line.starts_with("@@") {
            flush_blocks(&del_buf, &add_buf, &mut out);
            del_buf.clear();
            add_buf.clear();
            consecutive_blank_ctx = 0;

            if let Some((old_start, new_start)) = parse_hunk_header(raw_line) {
                old_line = old_start;
                new_line = new_start;
            }
            // Render a subtle separator between hunks (not before the first)
            if hunk_count > 0 {
                out.push(Line::from(Span::styled(
                    "        ⋯",
                    Style::default().fg(Color::DarkGray),
                )));
            }
            hunk_count += 1;
            continue;
        }

        let is_add = raw_line.starts_with('+');
        let is_remove = raw_line.starts_with('-');

        if is_add || is_remove {
            consecutive_blank_ctx = 0;
            changes_shown += 1;
            if changes_shown > max_changes {
                if is_add {
                    new_line += 1;
                } else {
                    old_line += 1;
                }
                continue;
            }
        } else if changes_shown > max_changes {
            new_line += 1;
            old_line += 1;
            continue;
        }

        if is_remove {
            // If we were accumulating adds, flush before starting dels
            if !add_buf.is_empty() {
                flush_blocks(&del_buf, &add_buf, &mut out);
                del_buf.clear();
                add_buf.clear();
            }
            let code = raw_line[1..].to_string();
            del_buf.push((code, old_line));
            old_line += 1;
        } else if is_add {
            let code = raw_line[1..].to_string();
            add_buf.push((code, new_line));
            new_line += 1;
        } else {
            // Context line — flush del/add buffers first
            flush_blocks(&del_buf, &add_buf, &mut out);
            del_buf.clear();
            add_buf.clear();

            let content = raw_line.strip_prefix(' ').unwrap_or(raw_line);
            if content.trim().is_empty() {
                consecutive_blank_ctx += 1;
                if consecutive_blank_ctx > 1 {
                    new_line += 1;
                    old_line += 1;
                    continue;
                }
            } else {
                consecutive_blank_ctx = 0;
            }

            let prefix = line_prefix(new_line, ' ', None);
            let code_line = ctx_line(content, &mut hl_ctx, ss);
            let mut spans = vec![prefix];
            spans.extend(code_line);
            out.push(Line::from(spans));
            new_line += 1;
            old_line += 1;
        }
    }

    // Flush any remaining del/add buffers
    flush_blocks(&del_buf, &add_buf, &mut out);

    if total_changes > max_changes {
        out.push(Line::from(Span::styled(
            format!(
                "     ... ({} more changes not shown)",
                total_changes - max_changes
            ),
            Style::default().fg(Color::DarkGray),
        )));
    }

    out
}

/// Render a context line with ANSI-16 syntax colours (no background).
fn ctx_line(
    code: &str,
    highlighter: &mut Option<HighlightLines<'_>>,
    ss: &syntect::parsing::SyntaxSet,
) -> Vec<Span<'static>> {
    let line_with_nl = format!("{code}\n");
    let ranges = highlighter
        .as_mut()
        .and_then(|h| h.highlight_line(&line_with_nl, ss).ok());

    match ranges {
        Some(tokens) => {
            let mut spans = Vec::new();
            for (style, text) in tokens {
                let text = text.trim_end_matches('\n');
                if text.is_empty() {
                    continue;
                }
                let fg = rgb_to_ansi16(style.foreground.r, style.foreground.g, style.foreground.b);
                spans.push(Span::styled(text.to_string(), Style::default().fg(fg)));
            }
            spans
        }
        None => vec![Span::styled(
            code.to_string(),
            Style::default().fg(Color::Gray),
        )],
    }
}

/// Map a syntect RGB colour to an ANSI-16 colour by analysing the hue.
fn rgb_to_ansi16(r: u8, g: u8, b: u8) -> Color {
    let max = r.max(g).max(b) as i16;
    let min = r.min(g).min(b) as i16;
    let chroma = max - min;

    if max < 90 {
        return Color::DarkGray;
    }
    if chroma < 30 {
        return Color::Reset;
    }

    let (r, g, b) = (r as i16, g as i16, b as i16);

    if r >= g && r >= b {
        if g >= b { Color::LightYellow } else { Color::LightMagenta }
    } else if g >= r && g >= b {
        if r >= b { Color::LightYellow } else { Color::LightGreen }
    } else if g >= r {
        Color::LightCyan
    } else {
        Color::LightBlue
    }
}
