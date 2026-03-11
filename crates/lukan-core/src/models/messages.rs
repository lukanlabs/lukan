use serde::{Deserialize, Serialize};

/// Message role in conversation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

/// A single content block within a message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        #[serde(rename = "toolUseId")]
        tool_use_id: String,
        content: String,
        #[serde(
            rename = "isError",
            alias = "is_error",
            skip_serializing_if = "Option::is_none"
        )]
        is_error: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        image: Option<String>,
    },
    Image {
        source: ImageSource,
        data: String,
        #[serde(
            rename = "mediaType",
            alias = "media_type",
            skip_serializing_if = "Option::is_none"
        )]
        media_type: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageSource {
    Base64,
    Url,
}

/// A message in the conversation history
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Message content can be a simple string or structured blocks
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl MessageContent {
    /// Extract text content from blocks or return the string directly
    pub fn to_text(&self) -> String {
        match self {
            MessageContent::Text(s) => s.clone(),
            MessageContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    ContentBlock::Thinking { text } => Some(text.as_str()),
                    ContentBlock::ToolResult { content, .. } => Some(content.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Get content blocks, wrapping text in a TextBlock if needed
    pub fn to_blocks(&self) -> Vec<ContentBlock> {
        match self {
            MessageContent::Text(s) => vec![ContentBlock::Text { text: s.clone() }],
            MessageContent::Blocks(blocks) => blocks.clone(),
        }
    }
}

impl Message {
    /// Create a user message with text content
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text(text.into()),
            tool_call_id: None,
            name: None,
        }
    }

    /// Create an assistant message with text content
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text(text.into()),
            tool_call_id: None,
            name: None,
        }
    }

    /// Create an assistant message with content blocks
    pub fn assistant_blocks(blocks: Vec<ContentBlock>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Blocks(blocks),
            tool_call_id: None,
            name: None,
        }
    }

    /// Create a tool result message
    pub fn tool_result(
        tool_use_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Blocks(vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
                is_error: if is_error { Some(true) } else { None },
                diff: None,
                image: None,
            }]),
            tool_call_id: None,
            name: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Role ────────────────────────────────────────────────────────

    #[test]
    fn test_role_serde() {
        assert_eq!(serde_json::to_string(&Role::User).unwrap(), r#""user""#);
        assert_eq!(
            serde_json::to_string(&Role::Assistant).unwrap(),
            r#""assistant""#
        );
        assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), r#""tool""#);

        let parsed: Role = serde_json::from_str(r#""user""#).unwrap();
        assert_eq!(parsed, Role::User);
    }

    // ── ImageSource ─────────────────────────────────────────────────

    #[test]
    fn test_image_source_serde() {
        assert_eq!(
            serde_json::to_string(&ImageSource::Base64).unwrap(),
            r#""base64""#
        );
        assert_eq!(
            serde_json::to_string(&ImageSource::Url).unwrap(),
            r#""url""#
        );
    }

    // ── ContentBlock ────────────────────────────────────────────────

    #[test]
    fn test_content_block_text_serde() {
        let block = ContentBlock::Text {
            text: "hello".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"text""#));
        assert!(json.contains(r#""text":"hello""#));

        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        match parsed {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_content_block_thinking_serde() {
        let block = ContentBlock::Thinking {
            text: "reasoning...".into(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"thinking""#));

        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        match parsed {
            ContentBlock::Thinking { text } => assert_eq!(text, "reasoning..."),
            _ => panic!("Expected Thinking variant"),
        }
    }

    #[test]
    fn test_content_block_tool_use_serde() {
        let block = ContentBlock::ToolUse {
            id: "tu-1".into(),
            name: "Bash".into(),
            input: serde_json::json!({"command": "ls"}),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"tool_use""#));
        assert!(json.contains(r#""name":"Bash""#));

        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        match parsed {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "tu-1");
                assert_eq!(name, "Bash");
                assert_eq!(input["command"], "ls");
            }
            _ => panic!("Expected ToolUse variant"),
        }
    }

    #[test]
    fn test_content_block_tool_result_serde() {
        let block = ContentBlock::ToolResult {
            tool_use_id: "tu-1".into(),
            content: "file1.txt\nfile2.txt".into(),
            is_error: Some(false),
            diff: None,
            image: None,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"tool_result""#));
        assert!(json.contains(r#""toolUseId":"tu-1""#));

        let parsed: ContentBlock = serde_json::from_str(&json).unwrap();
        match parsed {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                ..
            } => {
                assert_eq!(tool_use_id, "tu-1");
                assert_eq!(content, "file1.txt\nfile2.txt");
            }
            _ => panic!("Expected ToolResult variant"),
        }
    }

    #[test]
    fn test_content_block_tool_result_is_error_alias() {
        // Test that "is_error" (snake_case) is accepted as an alias
        let json = r#"{"type":"tool_result","toolUseId":"t1","content":"err","is_error":true}"#;
        let parsed: ContentBlock = serde_json::from_str(json).unwrap();
        match parsed {
            ContentBlock::ToolResult { is_error, .. } => assert_eq!(is_error, Some(true)),
            _ => panic!("Expected ToolResult"),
        }
    }

    // ── MessageContent ──────────────────────────────────────────────

    #[test]
    fn test_message_content_text_to_text() {
        let content = MessageContent::Text("hello world".into());
        assert_eq!(content.to_text(), "hello world");
    }

    #[test]
    fn test_message_content_blocks_to_text() {
        let content = MessageContent::Blocks(vec![
            ContentBlock::Text {
                text: "first".into(),
            },
            ContentBlock::Thinking {
                text: "thought".into(),
            },
            ContentBlock::ToolUse {
                id: "t1".into(),
                name: "Bash".into(),
                input: serde_json::json!({}),
            },
            ContentBlock::ToolResult {
                tool_use_id: "t1".into(),
                content: "result".into(),
                is_error: None,
                diff: None,
                image: None,
            },
        ]);
        let text = content.to_text();
        assert_eq!(text, "first\nthought\nresult");
    }

    #[test]
    fn test_message_content_to_blocks_from_text() {
        let content = MessageContent::Text("hello".into());
        let blocks = content.to_blocks();
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            ContentBlock::Text { text } => assert_eq!(text, "hello"),
            _ => panic!("Expected Text block"),
        }
    }

    #[test]
    fn test_message_content_to_blocks_from_blocks() {
        let blocks = vec![
            ContentBlock::Text { text: "a".into() },
            ContentBlock::Text { text: "b".into() },
        ];
        let content = MessageContent::Blocks(blocks.clone());
        let result = content.to_blocks();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_message_content_untagged_serde() {
        // Text variant
        let json = r#""hello""#;
        let parsed: MessageContent = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.to_text(), "hello");

        // Blocks variant
        let json = r#"[{"type":"text","text":"world"}]"#;
        let parsed: MessageContent = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.to_text(), "world");
    }

    // ── Message constructors ────────────────────────────────────────

    #[test]
    fn test_message_user() {
        let msg = Message::user("How are you?");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.content.to_text(), "How are you?");
        assert!(msg.tool_call_id.is_none());
        assert!(msg.name.is_none());
    }

    #[test]
    fn test_message_assistant() {
        let msg = Message::assistant("I'm fine!");
        assert_eq!(msg.role, Role::Assistant);
        assert_eq!(msg.content.to_text(), "I'm fine!");
    }

    #[test]
    fn test_message_assistant_blocks() {
        let blocks = vec![
            ContentBlock::Thinking {
                text: "let me think".into(),
            },
            ContentBlock::Text {
                text: "here is my answer".into(),
            },
        ];
        let msg = Message::assistant_blocks(blocks);
        assert_eq!(msg.role, Role::Assistant);
        assert!(msg.content.to_text().contains("let me think"));
        assert!(msg.content.to_text().contains("here is my answer"));
    }

    #[test]
    fn test_message_tool_result_success() {
        let msg = Message::tool_result("tu-123", "output text", false);
        assert_eq!(msg.role, Role::User);
        let blocks = msg.content.to_blocks();
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "tu-123");
                assert_eq!(content, "output text");
                assert!(is_error.is_none()); // None for success
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn test_message_tool_result_error() {
        let msg = Message::tool_result("tu-456", "error msg", true);
        let blocks = msg.content.to_blocks();
        match &blocks[0] {
            ContentBlock::ToolResult { is_error, .. } => {
                assert_eq!(*is_error, Some(true));
            }
            _ => panic!("Expected ToolResult"),
        }
    }

    #[test]
    fn test_message_serde_roundtrip() {
        let msg = Message::user("test message");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""role":"user""#));

        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, Role::User);
        assert_eq!(parsed.content.to_text(), "test message");
    }
}
