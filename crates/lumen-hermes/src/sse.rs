/// A decoded Server-Sent Event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// Optional explicit event name. Absent means default `message`.
    pub event: Option<String>,
    /// Concatenated data lines separated by `\n`.
    pub data: String,
}

/// Incremental SSE decoder resilient to arbitrary network chunking.
#[derive(Debug, Default)]
pub struct SseDecoder {
    buffer: String,
}

impl SseDecoder {
    /// Appends a text chunk and returns complete decoded events.
    pub fn push(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buffer.push_str(chunk);
        let normalized = self.buffer.replace("\r\n", "\n").replace('\r', "\n");
        let Some(last_sep) = normalized.rfind("\n\n") else {
            self.buffer = normalized;
            return Vec::new();
        };

        let complete = &normalized[..last_sep];
        let rest = normalized[last_sep + 2..].to_owned();
        self.buffer = rest;
        parse_sse_events(complete)
    }

    /// Flushes any final partial event.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        if self.buffer.trim().is_empty() {
            self.buffer.clear();
            return Vec::new();
        }
        let buffered = std::mem::take(&mut self.buffer);
        parse_sse_events(&buffered)
    }
}

/// Parses one or more complete SSE frames.
pub fn parse_sse_events(input: &str) -> Vec<SseEvent> {
    input.split("\n\n").filter_map(parse_frame).collect()
}

fn parse_frame(frame: &str) -> Option<SseEvent> {
    let mut event = None;
    let mut data = Vec::new();

    for raw_line in frame.lines() {
        let line = raw_line.trim_end();
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim_start().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.trim_start().to_owned());
        }
    }

    if event.is_none() && data.is_empty() {
        return None;
    }

    Some(SseEvent {
        event,
        data: data.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_and_default_events() {
        let events = parse_sse_events("event: tool.progress\ndata: {\"x\":1}\n\ndata: [DONE]\n\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.as_deref(), Some("tool.progress"));
        assert_eq!(events[0].data, "{\"x\":1}");
        assert_eq!(events[1].event, None);
        assert_eq!(events[1].data, "[DONE]");
    }

    #[test]
    fn handles_chunk_boundaries() {
        let mut decoder = SseDecoder::default();
        assert!(decoder.push("event: assistant.delta\nda").is_empty());
        let events = decoder.push("ta: hi\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("assistant.delta"));
        assert_eq!(events[0].data, "hi");
    }

    #[test]
    fn joins_multiline_data() {
        let events = parse_sse_events("data: hello\ndata: world\n\n");
        assert_eq!(events[0].data, "hello\nworld");
    }
}
