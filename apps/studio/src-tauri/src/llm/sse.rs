//! A minimal Server-Sent Events decoder: feed it bytes, get back each event's `data`.

#[derive(Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
    data: Vec<String>,
}

impl SseDecoder {
    /// Feeds a chunk and returns the data payloads of every event it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=pos).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            if line.is_empty() {
                if !self.data.is_empty() {
                    events.push(self.data.join("\n"));
                    self.data.clear();
                }
            } else if let Some(rest) = line.strip_prefix("data:") {
                self.data.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            }
            // Comments (`:`), `event:`, `id:` and `retry:` lines are not used by our providers.
        }
        events
    }

    /// Flushes an event left without a trailing blank line.
    pub fn finish(&mut self) -> Option<String> {
        if !self.buffer.is_empty() {
            let rest = String::from_utf8_lossy(&std::mem::take(&mut self.buffer)).into_owned();
            if let Some(d) = rest.trim_end().strip_prefix("data:") {
                self.data.push(d.trim_start().to_string());
            }
        }
        (!self.data.is_empty()).then(|| {
            let joined = self.data.join("\n");
            self.data.clear();
            joined
        })
    }
}

#[cfg(test)]
mod tests {
    use super::SseDecoder;

    #[test]
    fn splits_events_across_chunk_boundaries() {
        let mut d = SseDecoder::default();
        assert!(d.push(b"data: {\"a\"").is_empty());
        let out = d.push(b":1}\r\n\r\ndata: [DONE]\n\n: comment\n");
        assert_eq!(out, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
        assert!(d.finish().is_none());
    }

    #[test]
    fn joins_multiline_data_and_flushes_tail() {
        let mut d = SseDecoder::default();
        assert_eq!(d.push(b"data: one\ndata: two\n\n"), vec!["one\ntwo".to_string()]);
        d.push(b"data: tail");
        assert_eq!(d.finish().as_deref(), Some("tail"));
    }
}
