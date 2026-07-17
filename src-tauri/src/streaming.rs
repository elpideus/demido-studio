use anyhow::Result;
use bytes::Bytes;
use futures_util::StreamExt;
use reqwest::Response;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

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

    /// Like `next_data`, but also reports a raised cancel flag as soon as it is set.
    /// `next_data` alone only lets a caller observe the flag between chunks, so a quiet
    /// stream (a model thinking before it emits any delta) would ignore a stop until the
    /// next byte arrived — or forever.
    ///
    /// `Cancelled` is a normal outcome, not an error, so callers keep the partial output
    /// they have already accumulated rather than unwinding it away.
    pub async fn next_data_cancellable(&mut self, cancel: &Arc<AtomicBool>) -> Result<Chunk> {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Chunk::Cancelled);
        }
        let mut tick = tokio::time::interval(Duration::from_millis(50));
        tick.tick().await; // the first tick resolves immediately
        loop {
            tokio::select! {
                biased;
                r = self.next_data() => return Ok(match r? {
                    Some(d) => Chunk::Data(d),
                    None => Chunk::End,
                }),
                _ = tick.tick() => {
                    if cancel.load(Ordering::Relaxed) {
                        return Ok(Chunk::Cancelled);
                    }
                }
            }
        }
    }
}

/// One outcome of a cancellable SSE read.
pub enum Chunk {
    Data(String),
    /// The stream ended on its own.
    End,
    /// The user pressed stop. Whatever has been accumulated so far is still good.
    Cancelled,
}
