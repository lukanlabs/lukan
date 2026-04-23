use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph, Widget, Wrap},
};
use unicode_width::UnicodeWidthStr;

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
    streaming_thinking: &str,
    streaming_text: &str,
) -> Vec<Line<'static>> {
    build_message_lines_wide(messages, streaming_thinking, streaming_text, 0)
}

/// Same as `build_message_lines` but with an explicit render width so rows can
/// be padded (e.g. thinking block fills the row with its background).
pub fn build_message_lines_wide(
    messages: &[ChatMessage],
    streaming_thinking: &str,
    streaming_text: &str,
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for msg in messages {
        match msg.role.as_str() {
            "thinking" => {
                push_thinking_lines(&mut lines, &msg.content, width);
            }
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
            "notify" => {
                // Compact one-liner notification: yellow prefix + gray text
                lines.push(Line::from(vec![
                    Span::styled("▸ ", Style::default().fg(Color::Yellow)),
                    Span::styled(
                        msg.content.lines().next().unwrap_or("").to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]));
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
                push_user_lines(&mut lines, &msg.content, width);
            }
            _ => {
                // assistant — render markdown with styles & syntax highlighting
                lines.extend(render_markdown(&msg.content));
                lines.push(Line::from(""));
            }
        }
    }

    // Streaming thinking — render above the live text with the same style as
    // the finalized "thinking" role so the UI doesn't jump when it flushes.
    if !streaming_thinking.is_empty() {
        let sanitized = sanitize_for_display(streaming_thinking);
        push_thinking_lines(&mut lines, &sanitized, width);
        // Drop the trailing blank line when followed by streaming text so the
        // block hugs the response.
        if !streaming_text.is_empty()
            && lines
                .last()
                .is_some_and(|l| l.spans.iter().all(|s| s.content.is_empty()))
        {
            lines.pop();
        }
    }

    // Streaming text — render markdown with styles & syntax highlighting
    if !streaming_text.is_empty() {
        let sanitized = sanitize_for_display(streaming_text);
        lines.extend(render_markdown(&sanitized));
    }

    lines
}

// ── Bubble helpers (user message + thinking block) ─────────────────────

const USER_BG: Color = Color::Rgb(30, 30, 34);
const USER_BORDER: Color = Color::Rgb(235, 140, 55); // soft orange
/// Horizontal breathing room on the right so bubbles don't hug the terminal
/// edge — the gap shows the app backdrop instead.
const BUBBLE_RIGHT_GUTTER: u16 = 8;
/// Rows of same-bg padding on top AND bottom inside each bubble. Set to 0
/// so only the content line has the tinted background — no extra pad rows
/// above or below (user preference).
const BUBBLE_VERTICAL_PAD: usize = 1;
/// Rows of app backdrop appended after each bubble so the next message has
/// breathing room (requested by the user — separates assistant/next content).
const BUBBLE_BOTTOM_GAP: usize = 2;

/// Build a user-bubble row with the same structure as a normal content line.
/// Using a real text cell (even a single space) avoids the visual seam that
/// appeared when the padding rows were rendered with an empty text span.
fn build_user_bubble_row(
    border_style: Style,
    bg: Style,
    text_style: Style,
    box_width: u16,
    text: &str,
) -> Line<'static> {
    let mut spans = vec![
        Span::styled(" ", border_style),
        Span::styled("  ", bg),
        Span::styled(text.to_string(), text_style),
    ];
    let used = 3 + UnicodeWidthStr::width(text) as u16;
    pad_to_width(&mut spans, used, box_width, bg);
    Line::from(spans).style(bg)
}

/// Render the user's message with a colored left accent + filled dark
/// background, with internal vertical padding and an external backdrop gap
/// below so the next message has breathing room.
fn push_user_lines(lines: &mut Vec<Line<'static>>, content: &str, width: u16) {
    let bg_only = Style::default().bg(USER_BG);
    let border_style = Style::default().bg(USER_BORDER);
    let text_style = Style::default().fg(Color::Gray).bg(USER_BG);

    // Trim whitespace from BOTH ends so stray leading/trailing newlines don't
    // become empty rows inside the bubble.
    let content = content.trim_matches(|c: char| c.is_whitespace());

    // Paint the user bg all the way to the right edge so it matches the
    // thinking bubble width (uniform look).
    let box_width = width;

    let pad_row = build_user_bubble_row(border_style, bg_only, text_style, box_width, " ");

    if BUBBLE_VERTICAL_PAD > 0 {
        // Top pad
        for _ in 0..BUBBLE_VERTICAL_PAD {
            lines.push(pad_row.clone());
        }
    }

    // Collapse runs of blank lines so an unexpected empty line in the middle
    // of the content doesn't render as a full-row gap inside the bubble.
    // Pre-wrap so long pastes render as multiple full-bg rows instead of a
    // single line that Paragraph wraps (losing the bg on the overflow).
    let content_width = box_width.saturating_sub(3); // account for the 3-col left gutter
    let mut prev_blank = false;
    for raw_line in content.lines() {
        let line = raw_line.trim_end();
        let is_blank = line.is_empty();
        if is_blank && prev_blank {
            continue;
        }
        prev_blank = is_blank;
        let segments = if line.is_empty() {
            vec![String::new()]
        } else {
            wrap_text_to_width(line, content_width)
        };
        for segment in segments {
            lines.push(build_user_bubble_row(
                border_style,
                bg_only,
                text_style,
                box_width,
                &segment,
            ));
        }
    }

    if BUBBLE_VERTICAL_PAD > 0 {
        // Bottom pad — contiguous with the content (same USER_BG).
        for _ in 0..BUBBLE_VERTICAL_PAD {
            lines.push(pad_row.clone());
        }
    }

    // Backdrop gap below so the next message has breathing room.
    for _ in 0..BUBBLE_BOTTOM_GAP {
        lines.push(Line::from(""));
    }
}

// ── Thinking block helpers ─────────────────────────────────────────────

const THINKING_BG: Color = Color::Rgb(40, 40, 40);

fn thinking_base_style() -> Style {
    Style::default()
        .fg(Color::Gray)
        .bg(THINKING_BG)
        .add_modifier(Modifier::ITALIC)
}

/// Parse `**bold**` inline markers and emit styled spans. Preserves italic +
/// gray fg + gray bg, adds BOLD where the `**...**` wraps text.
fn parse_thinking_inline(text: &str) -> Vec<Span<'static>> {
    let base = thinking_base_style();
    let bold = base.add_modifier(Modifier::BOLD);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let mut plain_start = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            // Look for the closing "**"
            if let Some(rel) = find_double_star(&text[i + 2..]) {
                let close = i + 2 + rel;
                if close > i + 2 {
                    // Flush plain segment before the bold opener.
                    if plain_start < i {
                        spans.push(Span::styled(text[plain_start..i].to_string(), base));
                    }
                    spans.push(Span::styled(text[i + 2..close].to_string(), bold));
                    i = close + 2;
                    plain_start = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    if plain_start < text.len() {
        spans.push(Span::styled(text[plain_start..].to_string(), base));
    }
    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base));
    }
    spans
}

fn find_double_star(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut i = 0usize;
    while i + 1 < b.len() {
        if b[i] == b'*' && b[i + 1] == b'*' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Pad a line of spans with a trailing space span so the background extends
/// to the right edge. `used` is the displayed width of the existing spans.
fn pad_to_width(spans: &mut Vec<Span<'static>>, used: u16, width: u16, style: Style) {
    if width <= used {
        return;
    }
    let fill = (width - used) as usize;
    spans.push(Span::styled(" ".repeat(fill), style));
}

/// Soft-wrap `text` so every returned fragment fits within `max_width`
/// terminal columns. Splits on whitespace boundaries when possible, falls
/// back to hard-breaking mid-word when a single token is longer than the
/// line. Preserves empty segments for blank lines in the source.
fn wrap_text_to_width(text: &str, max_width: u16) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    let max = max_width as usize;
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_w: usize = 0;

    let mut word = String::new();
    let mut word_w: usize = 0;

    let flush_word = |current: &mut String,
                      current_w: &mut usize,
                      word: &mut String,
                      word_w: &mut usize,
                      out: &mut Vec<String>| {
        if word.is_empty() {
            return;
        }
        if *word_w > max {
            // Word longer than the full line — hard-break it.
            if !current.is_empty() {
                out.push(std::mem::take(current));
                *current_w = 0;
            }
            let mut piece = String::new();
            let mut piece_w: usize = 0;
            for ch in word.chars() {
                let cw = UnicodeWidthStr::width(ch.to_string().as_str());
                if piece_w + cw > max && !piece.is_empty() {
                    out.push(std::mem::take(&mut piece));
                    piece_w = 0;
                }
                piece.push(ch);
                piece_w += cw;
            }
            if !piece.is_empty() {
                *current = piece;
                *current_w = piece_w;
            }
            word.clear();
            *word_w = 0;
            return;
        }
        let extra = if current.is_empty() {
            *word_w
        } else {
            1 + *word_w
        };
        if *current_w + extra > max {
            out.push(std::mem::take(current));
            *current_w = 0;
        }
        if !current.is_empty() {
            current.push(' ');
            *current_w += 1;
        }
        current.push_str(word);
        *current_w += *word_w;
        word.clear();
        *word_w = 0;
    };

    for ch in text.chars() {
        if ch == ' ' || ch == '\t' {
            flush_word(
                &mut current,
                &mut current_w,
                &mut word,
                &mut word_w,
                &mut out,
            );
        } else {
            word.push(ch);
            word_w += UnicodeWidthStr::width(ch.to_string().as_str());
        }
    }
    flush_word(
        &mut current,
        &mut current_w,
        &mut word,
        &mut word_w,
        &mut out,
    );

    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Render the "Thinking:" header + content lines with full-row background.
/// Styled uniformly with the user bubble: right gutter + one row of same-bg
/// padding above and below, then a backdrop gap.
fn push_thinking_lines(lines: &mut Vec<Line<'static>>, content: &str, width: u16) {
    let base = thinking_base_style();
    let bg = Style::default().bg(THINKING_BG);
    let header = base.add_modifier(Modifier::BOLD);

    let content = content.trim_end_matches(['\n', '\r', ' ']);
    // Paint the thinking bg all the way to the right edge (no gutter).
    let box_width = width;

    let blank_row = {
        let mut spans = vec![Span::styled("   ", bg), Span::styled(String::new(), base)];
        pad_to_width(&mut spans, 3, box_width, bg);
        Line::from(spans).style(bg)
    };

    if BUBBLE_VERTICAL_PAD > 0 {
        // Top pad
        for _ in 0..BUBBLE_VERTICAL_PAD {
            lines.push(blank_row.clone());
        }
    }

    // Header row — indented with 3 spaces to match the bubble's left gutter.
    let mut header_spans = vec![Span::styled("   ", bg), Span::styled("Thinking:", header)];
    let header_used = 3 + "Thinking:".len() as u16;
    pad_to_width(&mut header_spans, header_used, box_width, bg);
    lines.push(Line::from(header_spans).style(bg));

    // Content rows — prefix with 3 spaces of bg to keep consistent left gutter.
    // Pre-wrap so long paragraphs render as multiple full-bg rows instead of
    // a single line that Paragraph wraps (losing the bg on the overflow).
    let content_width = box_width.saturating_sub(3); // account for the 3-col left gutter
    for line in content.lines() {
        let wrapped = wrap_text_to_width(line, content_width);
        for segment in wrapped {
            let mut spans = vec![Span::styled("   ", bg)];
            spans.extend(parse_thinking_inline(&segment));
            let used: u16 = spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()) as u16)
                .sum();
            pad_to_width(&mut spans, used, box_width, bg);
            lines.push(Line::from(spans).style(bg));
        }
    }

    if BUBBLE_VERTICAL_PAD > 0 {
        // Bottom pad
        for _ in 0..BUBBLE_VERTICAL_PAD {
            lines.push(blank_row.clone());
        }
    }

    // Backdrop gap so the assistant response below is clearly separated.
    for _ in 0..BUBBLE_BOTTOM_GAP {
        lines.push(Line::from(""));
    }
}

/// Count physical rows that `lines` would occupy when wrapped at `width`.
/// Uses ratatui's own `Paragraph::line_count` so the result exactly matches
/// what `Paragraph::new(lines).wrap(Wrap { trim: false })` actually renders
/// (including word-wrapping behaviour that can add extra rows).
pub fn physical_row_count(lines: &[Line], width: u16) -> u16 {
    if lines.is_empty() || width == 0 {
        return 0;
    }
    let paragraph = Paragraph::new(lines.to_vec()).wrap(Wrap { trim: false });
    paragraph.line_count(width) as u16
}

/// Widget that renders the chat history
pub struct ChatWidget<'a> {
    messages: &'a [ChatMessage],
    streaming_thinking: &'a str,
    streaming_text: &'a str,
    /// Whether older content has been pushed to terminal scrollback.
    has_scrollback: bool,
    /// Rows already pushed to scrollback from current uncommitted content.
    /// ChatWidget skips these rows so they don't render twice.
    scroll_offset: u16,
}

impl<'a> ChatWidget<'a> {
    pub fn new(
        messages: &'a [ChatMessage],
        streaming_thinking: &'a str,
        streaming_text: &'a str,
        has_scrollback: bool,
        scroll_offset: u16,
    ) -> Self {
        Self {
            messages,
            streaming_thinking,
            streaming_text,
            has_scrollback,
            scroll_offset,
        }
    }
}

impl Widget for ChatWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Clear the entire area first — ratatui double-buffers and Paragraph
        // only writes cells where it has text. Without this, characters from
        // previous frames bleed through as ghost text.
        Clear.render(area, buf);

        let lines = build_message_lines_wide(
            self.messages,
            self.streaming_thinking,
            self.streaming_text,
            area.width,
        );
        let total_rows = physical_row_count(&lines, area.width);

        if self.scroll_offset > 0 {
            // Some rows are already in scrollback — skip them
            let paragraph = Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll_offset, 0));
            paragraph.render(area, buf);
        } else if total_rows > area.height {
            // Overflow not yet pushed to scrollback (edge case / first frame)
            let skip = total_rows - area.height;
            let paragraph = Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .scroll((skip, 0));
            paragraph.render(area, buf);
        } else if self.has_scrollback {
            // Content fits AND there's scrollback above: render from the TOP
            let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
            paragraph.render(area, buf);
        } else {
            // Content fits, no scrollback: bottom-anchor near the input line
            let top_padding = area.height - total_rows;
            let bottom_area = Rect {
                y: area.y + top_padding,
                height: total_rows.max(1),
                ..area
            };
            let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
            paragraph.render(bottom_area, buf);
        }
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
            spans.push(Span::styled(
                buf.clone(),
                Style::default().fg(Color::White).bg(b),
            ));
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
        if g >= b {
            Color::LightYellow
        } else {
            Color::LightMagenta
        }
    } else if g >= r && g >= b {
        if r >= b {
            Color::LightYellow
        } else {
            Color::LightGreen
        }
    } else if g >= r {
        Color::LightCyan
    } else {
        Color::LightBlue
    }
}
