//! Vision preprocessor: when the active provider doesn't support images,
//! use a separate vision-capable model to describe them as text.

use std::collections::HashMap;
use std::sync::Arc;

use lukan_core::models::events::StreamEvent;
use lukan_core::models::messages::{ContentBlock, ImageSource, Message, MessageContent, Role};
use lukan_providers::{Provider, StreamParams, SystemPrompt};
use tokio::sync::mpsc;
use tracing::warn;

const VISION_SYSTEM: &str = "You are a vision assistant. Describe images accurately and concisely.";

const VISION_USER_PROMPT: &str = "Describe this image in detail. Include all visible text, UI elements, layout, colors, and any relevant information. Be thorough but concise.";

/// Scan `messages` for images. If the active provider can handle them, return
/// a clone unchanged. Otherwise describe every image via `vision_provider`
/// (or replace with a placeholder when no vision provider is available).
pub async fn preprocess_images(
    messages: &[Message],
    provider: &dyn Provider,
    vision_provider: &Option<Arc<dyn Provider>>,
) -> Vec<Message> {
    // Fast path: provider handles images natively
    if provider.supports_images() {
        return messages.to_vec();
    }

    // Quick scan: anything to do?
    if !has_images(messages) {
        return messages.to_vec();
    }

    // Collect unique images → description (dedup by first 100 chars of data)
    let mut cache: HashMap<String, String> = HashMap::new();

    let mut out = Vec::with_capacity(messages.len());
    for msg in messages {
        out.push(rewrite_message(msg, vision_provider, &mut cache).await);
    }
    out
}

/// Returns true if any message contains an image block or a tool result with an image.
fn has_images(messages: &[Message]) -> bool {
    for msg in messages {
        let blocks = match &msg.content {
            MessageContent::Text(_) => continue,
            MessageContent::Blocks(b) => b,
        };
        for block in blocks {
            match block {
                ContentBlock::Image { .. } => return true,
                ContentBlock::ToolResult { image: Some(_), .. } => return true,
                _ => {}
            }
        }
    }
    false
}

/// Dedup key: first 100 chars of the image data.
fn dedup_key(data: &str) -> String {
    data.chars().take(100).collect()
}

/// Rewrite a single message, replacing images with text descriptions.
async fn rewrite_message(
    msg: &Message,
    vision_provider: &Option<Arc<dyn Provider>>,
    cache: &mut HashMap<String, String>,
) -> Message {
    let blocks = match &msg.content {
        MessageContent::Text(_) => return msg.clone(),
        MessageContent::Blocks(b) => b,
    };

    let mut new_blocks = Vec::with_capacity(blocks.len());
    for block in blocks {
        match block {
            ContentBlock::Image { data, .. } => {
                let desc = describe_image_cached(block, data, vision_provider, cache).await;
                new_blocks.push(ContentBlock::Text { text: desc });
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
                diff,
                image: Some(image_data),
            } => {
                let desc = describe_data_url_cached(image_data, vision_provider, cache).await;
                let mut new_content = content.clone();
                if !new_content.is_empty() {
                    new_content.push('\n');
                }
                new_content.push_str(&desc);
                new_blocks.push(ContentBlock::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    content: new_content,
                    is_error: *is_error,
                    diff: diff.clone(),
                    image: None,
                });
            }
            other => new_blocks.push(other.clone()),
        }
    }

    Message {
        role: msg.role.clone(),
        content: MessageContent::Blocks(new_blocks),
        tool_call_id: msg.tool_call_id.clone(),
        name: msg.name.clone(),
    }
}

/// Describe an Image content block, using cache to avoid duplicate calls.
async fn describe_image_cached(
    block: &ContentBlock,
    data: &str,
    vision_provider: &Option<Arc<dyn Provider>>,
    cache: &mut HashMap<String, String>,
) -> String {
    let key = dedup_key(data);
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }
    let desc = match vision_provider {
        Some(vp) => describe_with_provider(vp.as_ref(), block.clone()).await,
        None => "[Image removed — no vision model configured]".to_string(),
    };
    cache.insert(key, desc.clone());
    desc
}

/// Describe a data URL image from a ToolResult, using cache.
async fn describe_data_url_cached(
    data_url: &str,
    vision_provider: &Option<Arc<dyn Provider>>,
    cache: &mut HashMap<String, String>,
) -> String {
    let key = dedup_key(data_url);
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }
    let desc = match vision_provider {
        Some(vp) => {
            // Parse data URL: "data:image/png;base64,<data>"
            let block = parse_data_url_to_image_block(data_url);
            describe_with_provider(vp.as_ref(), block).await
        }
        None => "[Image removed — no vision model configured]".to_string(),
    };
    cache.insert(key, desc.clone());
    desc
}

/// Parse a data URL into an Image content block.
fn parse_data_url_to_image_block(data_url: &str) -> ContentBlock {
    // Format: "data:image/png;base64,<base64data>"
    let media_type = data_url
        .strip_prefix("data:")
        .and_then(|s| s.split(';').next())
        .map(|s| s.to_string());

    let data = data_url
        .find(',')
        .map(|i| &data_url[i + 1..])
        .unwrap_or(data_url)
        .to_string();

    ContentBlock::Image {
        source: ImageSource::Base64,
        data,
        media_type,
    }
}

/// Call the vision provider to describe a single image.
/// Applies a 30-second timeout to prevent hanging indefinitely.
async fn describe_with_provider(provider: &dyn Provider, image_block: ContentBlock) -> String {
    match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        describe_with_provider_inner(provider, image_block),
    )
    .await
    {
        Ok(desc) => desc,
        Err(_) => {
            warn!("Vision provider timed out after 30s");
            "[Image description unavailable — vision model timed out]".to_string()
        }
    }
}

async fn describe_with_provider_inner(
    provider: &dyn Provider,
    image_block: ContentBlock,
) -> String {
    let user_msg = Message {
        role: Role::User,
        content: MessageContent::Blocks(vec![
            image_block,
            ContentBlock::Text {
                text: VISION_USER_PROMPT.to_string(),
            },
        ]),
        tool_call_id: None,
        name: None,
    };

    let params = StreamParams {
        system_prompt: SystemPrompt::Text(VISION_SYSTEM.to_string()),
        messages: vec![user_msg],
        tools: vec![],
    };

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(64);

    let provider_name = provider.name().to_string();
    if let Err(e) = provider.stream(params, tx).await {
        warn!("Vision provider ({provider_name}) error: {e}");
        return format!("[Image description unavailable — vision model error: {e}]");
    }

    // Collect text from stream events
    let mut description = String::new();
    while let Some(event) = rx.recv().await {
        match event {
            StreamEvent::TextDelta { text } => description.push_str(&text),
            StreamEvent::MessageEnd { .. } => break,
            StreamEvent::Error { error } => {
                warn!("Vision provider stream error: {error}");
                if description.is_empty() {
                    return format!("[Image description unavailable — {error}]");
                }
                break;
            }
            _ => {}
        }
    }

    if description.is_empty() {
        "[Image description unavailable — empty response from vision model]".to_string()
    } else {
        format!("[Image description: {description}]")
    }
}
