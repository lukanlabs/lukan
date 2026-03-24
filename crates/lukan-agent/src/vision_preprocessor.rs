//! Vision preprocessor: when the active provider doesn't support images,
//! use a separate vision-capable model to describe them as text.

use std::collections::HashMap;
use std::sync::Arc;

use base64::Engine;
use lukan_core::models::events::StreamEvent;
use lukan_core::models::messages::{ContentBlock, ImageSource, Message, MessageContent, Role};
use lukan_providers::{Provider, StreamParams, SystemPrompt};
use regex::Regex;
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
    // Fast path: provider handles images natively — but still need to
    // extract data URL images from ToolResults into Image blocks
    if provider.supports_images() {
        return expand_tool_result_images(messages);
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

/// For vision-capable providers: expand ToolResult.image data URLs into
/// separate Image content blocks so the provider can see them.
fn expand_tool_result_images(messages: &[Message]) -> Vec<Message> {
    let mut out = Vec::with_capacity(messages.len());
    for msg in messages {
        let blocks = match &msg.content {
            MessageContent::Blocks(b) => b,
            _ => {
                out.push(msg.clone());
                continue;
            }
        };

        let mut needs_expand = false;
        for block in blocks {
            if let ContentBlock::ToolResult { image: Some(_), .. } = block {
                needs_expand = true;
                break;
            }
        }

        if !needs_expand {
            out.push(msg.clone());
            continue;
        }

        let mut new_blocks = Vec::with_capacity(blocks.len() + 1);
        for block in blocks {
            match block {
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    diff,
                    image: Some(data_url),
                } => {
                    // Keep the text tool result
                    new_blocks.push(ContentBlock::ToolResult {
                        tool_use_id: tool_use_id.clone(),
                        content: content.clone(),
                        is_error: *is_error,
                        diff: diff.clone(),
                        image: None,
                    });
                    // Add the image as a separate block
                    new_blocks.push(parse_data_url_to_image_block(data_url));
                }
                other => new_blocks.push(other.clone()),
            }
        }

        out.push(Message {
            role: msg.role.clone(),
            content: MessageContent::Blocks(new_blocks),
            tool_call_id: msg.tool_call_id.clone(),
            name: msg.name.clone(),
        });
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
async fn describe_with_provider(provider: &dyn Provider, image_block: ContentBlock) -> String {
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
    // We need to call the provider but it takes &self not Arc, so we do this in-task
    // The provider is Send+Sync so we can move the reference across the spawn boundary
    // by using a scoped approach. However, since Provider is behind a dyn ref we
    // cannot spawn. Instead, run inline (vision calls are short).
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

// ── Image URL extraction ───────────────────────────────────────────────────

/// Infer MIME type from a URL's file extension.
pub(crate) fn media_type_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or(url);
    let ext = path.rsplit('.').next()?.to_lowercase();
    match ext.as_str() {
        "png" => Some("image/png".into()),
        "jpg" | "jpeg" => Some("image/jpeg".into()),
        "gif" => Some("image/gif".into()),
        "webp" => Some("image/webp".into()),
        "svg" => Some("image/svg+xml".into()),
        "bmp" => Some("image/bmp".into()),
        "ico" => Some("image/x-icon".into()),
        _ => None,
    }
}

/// Flatten RGBA image to RGB with white background, re-encode as PNG.
/// Returns the original bytes unchanged if not RGBA or if processing fails.
pub(crate) fn flatten_alpha(bytes: &[u8]) -> Vec<u8> {
    let img = match image::load_from_memory(bytes) {
        Ok(img) => img,
        Err(_) => return bytes.to_vec(),
    };

    if !matches!(
        img.color(),
        image::ColorType::Rgba8 | image::ColorType::Rgba16
    ) {
        return bytes.to_vec();
    }

    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut rgb = image::RgbImage::new(w, h);
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = a as f32 / 255.0;
        let blend = |fg: u8| -> u8 { (fg as f32 * alpha + 255.0 * (1.0 - alpha)) as u8 };
        rgb.put_pixel(x, y, image::Rgb([blend(r), blend(g), blend(b)]));
    }

    let mut buf = std::io::Cursor::new(Vec::new());
    if rgb.write_to(&mut buf, image::ImageFormat::Png).is_ok() {
        buf.into_inner()
    } else {
        bytes.to_vec()
    }
}

/// Extract image URLs from user text, fetch them, and return cleaned text + base64 image blocks.
pub(crate) async fn extract_image_urls(text: &str) -> (String, Vec<ContentBlock>) {
    let re = Regex::new(r"https?://\S+\.(?:png|jpe?g|gif|webp|svg|bmp|ico)(?:\?\S*)?").unwrap();

    let urls: Vec<String> = re.find_iter(text).map(|m| m.as_str().to_string()).collect();
    if urls.is_empty() {
        return (text.to_string(), Vec::new());
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();

    let mut images = Vec::new();
    for url in &urls {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let media_type = resp
                    .headers()
                    .get("content-type")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.split(';').next().unwrap_or(s).trim().to_string())
                    .or_else(|| media_type_from_url(url));

                match resp.bytes().await {
                    Ok(bytes) => {
                        let processed = flatten_alpha(&bytes);
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&processed);
                        images.push(ContentBlock::Image {
                            source: ImageSource::Base64,
                            data: b64,
                            media_type,
                        });
                    }
                    Err(e) => warn!("Failed to read image bytes from {url}: {e}"),
                }
            }
            Ok(resp) => warn!("Failed to fetch image {url}: HTTP {}", resp.status()),
            Err(e) => warn!("Failed to fetch image {url}: {e}"),
        }
    }

    let cleaned = re.replace_all(text, "").trim().to_string();
    (cleaned, images)
}
