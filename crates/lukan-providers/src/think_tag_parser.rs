/// Parser for extracting <think>...</think> tags from streamed text.
///
/// Some providers (DeepSeek, etc.) embed reasoning in think tags.
/// This parser extracts thinking content from the text stream.

#[derive(Debug)]
pub enum ThinkTagOutput {
    /// Regular text content (outside think tags)
    Text(String),
    /// Thinking/reasoning content (inside think tags)
    Thinking(String),
}

/// Stateful parser that splits text stream into thinking and regular content
pub struct ThinkTagParser {
    buffer: String,
    inside_think: bool,
}

impl Default for ThinkTagParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ThinkTagParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            inside_think: false,
        }
    }

    /// Feed a text chunk and extract any complete segments
    pub fn feed(&mut self, chunk: &str) -> Vec<ThinkTagOutput> {
        self.buffer.push_str(chunk);
        let mut outputs = Vec::new();

        loop {
            if self.inside_think {
                if let Some(end_pos) = self.buffer.find("</think>") {
                    let thinking = self.buffer[..end_pos].to_string();
                    self.buffer = self.buffer[end_pos + 8..].to_string();
                    self.inside_think = false;
                    if !thinking.is_empty() {
                        outputs.push(ThinkTagOutput::Thinking(thinking));
                    }
                } else {
                    // Might have partial </think> at end — hold the buffer
                    break;
                }
            } else if let Some(start_pos) = self.buffer.find("<think>") {
                let text = self.buffer[..start_pos].to_string();
                self.buffer = self.buffer[start_pos + 7..].to_string();
                self.inside_think = true;
                if !text.is_empty() {
                    outputs.push(ThinkTagOutput::Text(text));
                }
            } else {
                // Check if we might have a partial <think> at the end.
                // We hold back up to 6 bytes ("<think" minus one char) so we
                // don't emit a partial tag.  The split point must land on a
                // UTF-8 char boundary — walk backwards to the nearest one.
                let target = self.buffer.len().saturating_sub(6);
                let safe_len = self.buffer.floor_char_boundary(target);
                if safe_len > 0 {
                    let text = self.buffer[..safe_len].to_string();
                    self.buffer = self.buffer[safe_len..].to_string();
                    outputs.push(ThinkTagOutput::Text(text));
                }
                break;
            }
        }

        outputs
    }

    /// Flush remaining buffer content
    pub fn flush(&mut self) -> Option<ThinkTagOutput> {
        if self.buffer.is_empty() {
            return None;
        }
        let content = std::mem::take(&mut self.buffer);
        if self.inside_think {
            Some(ThinkTagOutput::Thinking(content))
        } else {
            Some(ThinkTagOutput::Text(content))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_think_tags() {
        let mut parser = ThinkTagParser::new();
        let outputs = parser.feed("Hello world");
        // May hold partial buffer
        let flushed = parser.flush();
        let all_text: String = outputs
            .iter()
            .chain(flushed.iter())
            .filter_map(|o| match o {
                ThinkTagOutput::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(all_text, "Hello world");
    }

    #[test]
    fn test_think_tags() {
        let mut parser = ThinkTagParser::new();
        let outputs = parser.feed("Before<think>reasoning</think>After");
        let flushed = parser.flush();
        let all: Vec<_> = outputs.into_iter().chain(flushed).collect();

        let has_text_before = all
            .iter()
            .any(|o| matches!(o, ThinkTagOutput::Text(t) if t.contains("Before")));
        let has_thinking = all
            .iter()
            .any(|o| matches!(o, ThinkTagOutput::Thinking(t) if t == "reasoning"));
        let has_text_after = all
            .iter()
            .any(|o| matches!(o, ThinkTagOutput::Text(t) if t.contains("After")));

        assert!(has_text_before);
        assert!(has_thinking);
        assert!(has_text_after);
    }

    #[test]
    fn test_multibyte_no_panic() {
        // "¡Hola!" is 8 bytes (¡=2, H=1, o=1, l=1, a=1, !=1) — safe_len
        // would land inside the multi-byte ¡ if we don't respect char boundaries.
        let mut parser = ThinkTagParser::new();
        let outputs = parser.feed("¡Hola!");
        let flushed = parser.flush();
        let all_text: String = outputs
            .iter()
            .chain(flushed.iter())
            .filter_map(|o| match o {
                ThinkTagOutput::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(all_text, "¡Hola!");
    }

    #[test]
    fn test_multibyte_with_think_tags() {
        let mut parser = ThinkTagParser::new();
        let outputs = parser.feed("Héllo<think>résumé</think>café");
        let flushed = parser.flush();
        let all: Vec<_> = outputs.into_iter().chain(flushed).collect();

        let has_thinking = all
            .iter()
            .any(|o| matches!(o, ThinkTagOutput::Thinking(t) if t == "résumé"));
        assert!(has_thinking);
    }
}
