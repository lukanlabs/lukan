use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::app::ApprovalPrompt;

pub(crate) struct ApprovalPromptWidget<'a> {
    prompt: &'a ApprovalPrompt,
}

impl<'a> ApprovalPromptWidget<'a> {
    pub(crate) fn new(prompt: &'a ApprovalPrompt) -> Self {
        Self { prompt }
    }
}

impl Widget for ApprovalPromptWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let mut lines: Vec<Line<'_>> = Vec::new();

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            if self.prompt.all_read_only {
                " Tool Approval Required (read-only tools)"
            } else {
                " Tool Approval Required"
            },
            Style::default()
                .fg(if self.prompt.all_read_only {
                    Color::Green
                } else {
                    Color::Cyan
                })
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        for (i, tool) in self.prompt.tools.iter().enumerate() {
            let is_selected = i == self.prompt.selected;
            let is_checked = self.prompt.selections.get(i).copied().unwrap_or(false);

            let pointer = if is_selected { "▸ " } else { "  " };
            let checkbox = if is_checked { "[x] " } else { "[ ] " };

            let raw_summary = tool
                .activity_label
                .clone()
                .or_else(|| tool.search_hint.clone())
                .unwrap_or_else(|| summarize_tool_input(&tool.name, &tool.input));
            let prefer_concrete_summary = matches!(
                tool.name.as_str(),
                "Bash"
                    | "ReadFiles"
                    | "WriteFile"
                    | "EditFile"
                    | "WebFetch"
                    | "Explore"
                    | "SubAgent"
            );
            let summary = if prefer_concrete_summary {
                summarize_tool_input(&tool.name, &tool.input)
            } else {
                raw_summary
            };
            let metadata_suffix = if prefer_concrete_summary {
                match tool.read_only {
                    Some(true) => " [read-only]".to_string(),
                    _ => String::new(),
                }
            } else {
                match (tool.read_only, tool.search_hint.as_deref()) {
                    (Some(true), Some(hint)) => format!(" [read-only · {hint}]"),
                    (Some(true), None) => " [read-only]".to_string(),
                    (Some(false), Some(hint)) => format!(" [{hint}]"),
                    _ => String::new(),
                }
            };
            let label = format!(
                "{}{}",
                tool.name,
                if summary.len() > 60 {
                    let end = summary.floor_char_boundary(57);
                    format!("({}...){}", &summary[..end], metadata_suffix)
                } else {
                    format!("({summary}){metadata_suffix}")
                }
            );

            let style = if is_selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            let check_style = if is_checked {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            };

            lines.push(Line::from(vec![
                Span::styled(format!(" {pointer}"), style),
                Span::styled(checkbox.to_string(), check_style),
                Span::styled(label, style),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " Space toggle · Enter submit · a approve all · A always allow · Esc deny all",
            Style::default().fg(Color::DarkGray),
        )));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Tool Approval ")
            .border_style(Style::default().fg(Color::Yellow));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}

/// Produce a human-readable one-liner for the tool call input
pub(crate) fn summarize_tool_input(name: &str, input: &serde_json::Value) -> String {
    match name {
        "Bash" => input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("(no command)")
            .to_string(),
        "ReadFiles" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let mut s = path.to_string();
            if let Some(offset) = input.get("offset").and_then(|v| v.as_u64()) {
                s.push_str(&format!(" (from line {offset})"));
            }
            s
        }
        "WriteFile" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let len = input
                .get("content")
                .and_then(|v| v.as_str())
                .map(|c| c.len())
                .unwrap_or(0);
            format!("{path} ({len} bytes)")
        }
        "EditFile" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let replace_all = input
                .get("replace_all")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if replace_all {
                format!("{path} (replace all)")
            } else {
                path.to_string()
            }
        }
        "Grep" => {
            let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
            let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
            format!("{pattern} in {path}")
        }
        "Glob" => input
            .get("pattern")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        "WebFetch" => input
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string(),
        "Explore" | "SubAgent" => {
            let task = input.get("task").and_then(|v| v.as_str()).unwrap_or("?");
            if task.len() > 80 {
                let end = task.floor_char_boundary(80);
                format!("{}…", &task[..end])
            } else {
                task.to_string()
            }
        }
        _ => {
            // Fallback: compact JSON
            let s = serde_json::to_string(input).unwrap_or_default();
            if s.len() > 200 {
                let end = s.floor_char_boundary(200);
                format!("{}...", &s[..end])
            } else {
                s
            }
        }
    }
}
