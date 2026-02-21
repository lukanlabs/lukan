//! Parser for Server-Sent Events (SSE) streams.
//!
//! Handles the SSE protocol: lines starting with "data: " contain payload,
//! empty lines delimit events. "data: [DONE]" signals end of stream.

/// A parsed SSE event
#[derive(Debug)]
pub enum SseEvent {
    /// A data payload (the JSON string after "data: ")
    Data(String),
    /// End of stream marker
    Done,
}

/// Stateful SSE line parser
pub struct SseParser {
    buffer: String,
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// Feed a chunk of bytes and extract complete SSE events
    pub fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buffer.push_str(chunk);
        let mut events = Vec::new();

        while let Some(newline_pos) = self.buffer.find('\n') {
            let line = self.buffer[..newline_pos]
                .trim_end_matches('\r')
                .to_string();
            self.buffer = self.buffer[newline_pos + 1..].to_string();

            if line.is_empty() {
                // Empty line = event delimiter (we emit events on data lines directly)
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    events.push(SseEvent::Done);
                } else {
                    events.push(SseEvent::Data(data.to_string()));
                }
            }
            // Ignore "event:", "id:", "retry:" lines
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_data() {
        let mut parser = SseParser::new();
        let events = parser.feed("data: {\"type\":\"text\"}\n\n");
        assert_eq!(events.len(), 1);
        match &events[0] {
            SseEvent::Data(d) => assert_eq!(d, "{\"type\":\"text\"}"),
            _ => panic!("Expected Data event"),
        }
    }

    #[test]
    fn test_parse_sse_done() {
        let mut parser = SseParser::new();
        let events = parser.feed("data: [DONE]\n\n");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], SseEvent::Done));
    }

    #[test]
    fn test_parse_sse_partial_chunks() {
        let mut parser = SseParser::new();
        let events1 = parser.feed("data: {\"ty");
        assert!(events1.is_empty());
        let events2 = parser.feed("pe\":\"text\"}\n\n");
        assert_eq!(events2.len(), 1);
    }
}
