use anyhow::Result;
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::Response;

/// Reads SSE (Server-Sent Events) data lines from a streaming HTTP response.
/// Yields only `data: ...` lines, stripping the `data: ` prefix.
/// Skips blank lines, comments (`:`), and other event fields.
pub struct SseLineReader {
    inner: futures_util::stream::BoxStream<'static, reqwest::Result<Bytes>>,
    buffer: String,
}

impl SseLineReader {
    pub fn new(resp: Response) -> Self {
        Self {
            inner: Box::pin(resp.bytes_stream()),
            buffer: String::new(),
        }
    }

    /// Returns the next SSE data payload, or None if the stream ended.
    pub async fn next_data(&mut self) -> Result<Option<String>> {
        loop {
            // Process buffered complete lines first
            if let Some(pos) = self.buffer.find('\n') {
                let line = self.buffer[..pos].trim_end_matches('\r').to_string();
                self.buffer = self.buffer[pos + 1..].to_string();

                if let Some(data) = line.strip_prefix("data: ") {
                    return Ok(Some(data.to_string()));
                }
                // Skip blank lines, comments, and other SSE fields
                continue;
            }

            // Need more bytes from the network
            match self.inner.next().await {
                Some(Ok(chunk)) => {
                    self.buffer.push_str(&String::from_utf8_lossy(&chunk));
                }
                Some(Err(e)) => return Err(e.into()),
                None => {
                    // Stream ended — drain remaining buffer
                    if !self.buffer.is_empty() {
                        let line = std::mem::take(&mut self.buffer);
                        let line = line.trim_end_matches('\r');
                        if let Some(data) = line.strip_prefix("data: ") {
                            return Ok(Some(data.to_string()));
                        }
                    }
                    return Ok(None);
                }
            }
        }
    }
}
