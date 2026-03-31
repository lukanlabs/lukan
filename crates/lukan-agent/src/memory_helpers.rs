//! Helpers for memory compaction, context formatting, and memory file I/O.

use std::path::PathBuf;

use lukan_core::config::LukanPaths;
use lukan_core::models::messages::{ContentBlock, Message, MessageContent, Role};

/// Format messages into a text representation for compaction/memory LLM calls
pub(crate) fn format_messages_for_context(messages: &[Message]) -> String {
    let mut output = String::new();
    for msg in messages {
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
            Role::Tool => "Tool",
        };

        match &msg.content {
            MessageContent::Text(text) => {
                output.push_str(&format!("[{role}]: {text}\n\n"));
            }
            MessageContent::Blocks(blocks) => {
                for block in blocks {
                    match block {
                        ContentBlock::Text { text } => {
                            output.push_str(&format!("[{role}]: {text}\n\n"));
                        }
                        ContentBlock::Thinking { text } => {
                            output.push_str(&format!("[{role} thinking]: {text}\n\n"));
                        }
                        ContentBlock::ToolUse { name, input, .. } => {
                            let input_str = serde_json::to_string(input).unwrap_or_default();
                            let truncated = if input_str.len() > 500 {
                                let end = input_str.floor_char_boundary(500);
                                format!("{}...", &input_str[..end])
                            } else {
                                input_str
                            };
                            output.push_str(&format!("[{role} tool_use]: {name}({truncated})\n\n"));
                        }
                        ContentBlock::ToolResult {
                            content, is_error, ..
                        } => {
                            let prefix = if *is_error == Some(true) {
                                "ERROR"
                            } else {
                                "result"
                            };
                            // Truncate long tool results
                            let truncated = if content.len() > 2000 {
                                let end = content.floor_char_boundary(2000);
                                format!("{}...(truncated)", &content[..end])
                            } else {
                                content.clone()
                            };
                            output.push_str(&format!("[Tool {prefix}]: {truncated}\n\n"));
                        }
                        ContentBlock::Image { .. } => {
                            output.push_str(&format!("[{role}]: [Image]\n\n"));
                        }
                    }
                }
            }
        }
    }
    output
}

/// Extract a section between two markers from LLM output
pub(crate) fn extract_section(text: &str, start_marker: &str, end_marker: &str) -> Option<String> {
    let start = text.find(start_marker)?;
    let content_start = start + start_marker.len();
    let content = if end_marker.is_empty() {
        &text[content_start..]
    } else if let Some(end) = text[content_start..].find(end_marker) {
        &text[content_start..content_start + end]
    } else {
        &text[content_start..]
    };
    let trimmed = content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Resolve the active memory path: project memory if `.active` marker exists,
/// otherwise None. Global memory is never auto-updated — it's user-managed only.
pub(crate) fn active_memory_path() -> Option<PathBuf> {
    let active_marker = LukanPaths::project_memory_active_file();
    if active_marker.exists() {
        return Some(LukanPaths::project_memory_file());
    }
    None
}

// write_memory_file_to removed — structured memory system writes files directly
