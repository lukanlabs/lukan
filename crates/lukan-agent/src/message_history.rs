use std::collections::HashSet;

use lukan_core::models::messages::{ContentBlock, Message, MessageContent, Role};
use tracing::debug;

/// Manages conversation message history with sanitization
pub struct MessageHistory {
    messages: Vec<Message>,
}

impl MessageHistory {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    /// Add any message to history
    pub fn add(&mut self, msg: Message) {
        self.messages.push(msg);
    }

    /// Add a user text message
    pub fn add_user_message(&mut self, content: &str) {
        self.add(Message::user(content));
    }

    /// Add a user message with structured content blocks (e.g. text + images)
    pub fn add_user_blocks(&mut self, blocks: Vec<ContentBlock>) {
        self.add(Message {
            role: Role::User,
            content: MessageContent::Blocks(blocks),
            tool_call_id: None,
            name: None,
        });
    }

    /// Add an assistant message with content blocks
    pub fn add_assistant_blocks(&mut self, blocks: Vec<ContentBlock>) {
        if !blocks.is_empty() {
            self.add(Message::assistant_blocks(blocks));
        }
    }

    /// Add a tool result (as a User message with ToolResult blocks)
    pub fn add_tool_result(
        &mut self,
        tool_use_id: &str,
        content: &str,
        is_error: bool,
        diff: Option<String>,
    ) {
        self.add(Message {
            role: Role::User,
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: content.to_string(),
                is_error: if is_error { Some(true) } else { None },
                diff,
                image: None,
            }]),
            tool_call_id: None,
            name: None,
        });
    }

    /// Get a reference to all messages
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Truncate messages to keep only the first `len` entries
    pub fn truncate(&mut self, len: usize) {
        self.messages.truncate(len);
    }

    /// Clone messages for serialization
    pub fn to_json(&self) -> Vec<Message> {
        self.messages.clone()
    }

    /// Load messages from JSON (e.g. from a saved session), with sanitization
    pub fn load_from_json(&mut self, mut messages: Vec<Message>) {
        sanitize_orphaned_tool_use(&mut messages);
        self.messages = messages;
    }

}

impl Default for MessageHistory {
    fn default() -> Self {
        Self::new()
    }
}

/// Collect all tool_use IDs from a set of messages
fn collect_tool_use_ids(messages: &[Message]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for msg in messages {
        if let MessageContent::Blocks(blocks) = &msg.content {
            for block in blocks {
                if let ContentBlock::ToolUse { id, .. } = block {
                    ids.insert(id.clone());
                }
            }
        }
    }
    ids
}

/// Collect all tool_result tool_use_ids from a set of messages
fn collect_tool_result_ids(messages: &[Message]) -> HashSet<String> {
    let mut ids = HashSet::new();
    for msg in messages {
        if let MessageContent::Blocks(blocks) = &msg.content {
            for block in blocks {
                if let ContentBlock::ToolResult { tool_use_id, .. } = block {
                    ids.insert(tool_use_id.clone());
                }
            }
        }
    }
    ids
}

/// Fix orphaned tool_use blocks (those without matching tool_result).
/// Inserts a synthetic `[Cancelled]` result for each orphaned tool_use.
fn sanitize_orphaned_tool_use(messages: &mut Vec<Message>) {
    let tool_use_ids = collect_tool_use_ids(messages);
    let tool_result_ids = collect_tool_result_ids(messages);

    let orphaned: Vec<String> = tool_use_ids.difference(&tool_result_ids).cloned().collect();

    if orphaned.is_empty() {
        return;
    }

    debug!(count = orphaned.len(), "Fixing orphaned tool_use blocks");

    // For each orphaned tool_use, find the assistant message containing it
    // and insert a synthetic tool_result right after
    let mut insertions: Vec<(usize, Message)> = Vec::new();

    for (idx, msg) in messages.iter().enumerate() {
        if msg.role != Role::Assistant {
            continue;
        }
        if let MessageContent::Blocks(blocks) = &msg.content {
            let mut result_blocks = Vec::new();
            for block in blocks {
                if let ContentBlock::ToolUse { id, .. } = block
                    && orphaned.contains(id)
                {
                    result_blocks.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: "[Cancelled]".to_string(),
                        is_error: Some(true),
                        diff: None,
                        image: None,
                    });
                }
            }
            if !result_blocks.is_empty() {
                insertions.push((
                    idx + 1,
                    Message {
                        role: Role::User,
                        content: MessageContent::Blocks(result_blocks),
                        tool_call_id: None,
                        name: None,
                    },
                ));
            }
        }
    }

    // Insert in reverse order to keep indices valid
    for (idx, msg) in insertions.into_iter().rev() {
        messages.insert(idx, msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_add_and_retrieve() {
        let mut history = MessageHistory::new();
        history.add_user_message("hello");
        history.add_assistant_blocks(vec![ContentBlock::Text {
            text: "hi".to_string(),
        }]);
        assert_eq!(history.messages().len(), 2);
    }

    #[test]
    fn test_clear() {
        let mut history = MessageHistory::new();
        history.add_user_message("hello");
        history.clear();
        assert!(history.messages().is_empty());
    }

    #[test]
    fn test_sanitize_orphaned_tool_use() {
        let mut messages = vec![
            Message::user("do something"),
            Message::assistant_blocks(vec![
                ContentBlock::Text {
                    text: "let me help".to_string(),
                },
                ContentBlock::ToolUse {
                    id: "tool_1".to_string(),
                    name: "Bash".to_string(),
                    input: json!({"command": "ls"}),
                },
            ]),
            // No tool_result for tool_1 — orphaned!
        ];

        sanitize_orphaned_tool_use(&mut messages);

        // Should now have 3 messages: user, assistant, synthetic tool_result
        assert_eq!(messages.len(), 3);
        if let MessageContent::Blocks(blocks) = &messages[2].content {
            assert_eq!(blocks.len(), 1);
            if let ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
                ..
            } = &blocks[0]
            {
                assert_eq!(tool_use_id, "tool_1");
                assert_eq!(content, "[Cancelled]");
                assert_eq!(*is_error, Some(true));
            } else {
                panic!("Expected ToolResult block");
            }
        } else {
            panic!("Expected Blocks content");
        }
    }

    #[test]
    fn test_no_sanitize_when_result_exists() {
        let mut messages = vec![
            Message::user("do something"),
            Message::assistant_blocks(vec![ContentBlock::ToolUse {
                id: "tool_1".to_string(),
                name: "Bash".to_string(),
                input: json!({"command": "ls"}),
            }]),
            Message {
                role: Role::User,
                content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "tool_1".to_string(),
                    content: "file.txt".to_string(),
                    is_error: None,
                    diff: None,
                    image: None,
                }]),
                tool_call_id: None,
                name: None,
            },
        ];

        let before_len = messages.len();
        sanitize_orphaned_tool_use(&mut messages);
        assert_eq!(messages.len(), before_len); // No change
    }

    #[test]
    fn test_to_json_and_load() {
        let mut history = MessageHistory::new();
        history.add_user_message("hello");
        history.add_assistant_blocks(vec![ContentBlock::Text {
            text: "hi".to_string(),
        }]);

        let json = history.to_json();
        let mut history2 = MessageHistory::new();
        history2.load_from_json(json);
        assert_eq!(history2.messages().len(), 2);
    }
}
