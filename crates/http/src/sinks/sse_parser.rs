//! Server-Sent Events (SSE) parser for streaming responses

use bytes::Bytes;
use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

/// SSE event parsed from the stream
#[derive(Debug, Clone, Default)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

/// Parser state for SSE stream
pub struct SseParser<S> {
    stream: S,
    buffer: String,
    current_event: SseEvent,
}

impl<S> SseParser<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            buffer: String::new(),
            current_event: SseEvent::default(),
        }
    }

    fn parse_line(&mut self, line: &str) -> Option<SseEvent> {
        // Empty line signals end of event
        if line.is_empty() {
            if !self.current_event.data.is_empty() {
                // Remove trailing newline from data
                if self.current_event.data.ends_with('\n') {
                    self.current_event.data.pop();
                }
                let event = std::mem::take(&mut self.current_event);
                return Some(event);
            }
            return None;
        }

        // Comment line
        if line.starts_with(':') {
            return None;
        }

        // Parse field
        let (field, value) = if let Some(colon_pos) = line.find(':') {
            let field = &line[..colon_pos];
            let mut value = &line[colon_pos + 1..];
            // Remove single leading space if present
            if value.starts_with(' ') {
                value = &value[1..];
            }
            (field, value)
        } else {
            (line, "")
        };

        match field {
            "event" => self.current_event.event = Some(value.to_string()),
            "data" => {
                if !self.current_event.data.is_empty() {
                    self.current_event.data.push('\n');
                }
                self.current_event.data.push_str(value);
            }
            "id" => self.current_event.id = Some(value.to_string()),
            "retry" => {
                if let Ok(retry) = value.parse::<u64>() {
                    self.current_event.retry = Some(retry);
                }
            }
            _ => {} // Ignore unknown fields
        }

        None
    }
}

impl<S> Stream for SseParser<S>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<SseEvent, reqwest::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // First, try to parse any complete events from the buffer
            while let Some(newline_pos) = self.buffer.find('\n') {
                let line = self.buffer.drain(..=newline_pos).collect::<String>();
                let line = line.trim_end_matches('\n').trim_end_matches('\r');

                if let Some(event) = self.parse_line(line) {
                    return Poll::Ready(Some(Ok(event)));
                }
            }

            // Need more data
            match Pin::new(&mut self.stream).poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    // Append new data to buffer
                    self.buffer.push_str(&String::from_utf8_lossy(&bytes));
                    // Continue loop to parse
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(e)));
                }
                Poll::Ready(None) => {
                    // Stream ended - process any remaining buffer
                    if !self.buffer.is_empty() {
                        let remaining = std::mem::take(&mut self.buffer);
                        if let Some(event) = self.parse_line(&remaining) {
                            return Poll::Ready(Some(Ok(event)));
                        }
                    }
                    // Check if we have a partial event
                    if !self.current_event.data.is_empty() {
                        let event = std::mem::take(&mut self.current_event);
                        return Poll::Ready(Some(Ok(event)));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// Parse an SSE stream into events
pub fn parse_sse<S>(stream: S) -> impl Stream<Item = Result<SseEvent, reqwest::Error>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    SseParser::new(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{StreamExt, stream};

    #[tokio::test]
    async fn test_parse_simple_event() {
        let data = b"data: hello world\n\n";
        let stream = stream::once(async { Ok(Bytes::from(&data[..])) });
        let stream = Box::pin(stream);
        let mut parser = SseParser::new(stream);

        let event = parser.next().await.unwrap().unwrap();
        assert_eq!(event.data, "hello world");
        assert_eq!(event.event, None);
    }

    #[tokio::test]
    async fn test_parse_multiline_data() {
        let data = b"data: line 1\ndata: line 2\n\n";
        let stream = stream::once(async { Ok(Bytes::from(&data[..])) });
        let stream = Box::pin(stream);
        let mut parser = SseParser::new(stream);

        let event = parser.next().await.unwrap().unwrap();
        assert_eq!(event.data, "line 1\nline 2");
    }

    #[tokio::test]
    async fn test_parse_with_event_type() {
        let data = b"event: message\ndata: hello\n\n";
        let stream = stream::once(async { Ok(Bytes::from(&data[..])) });
        let stream = Box::pin(stream);
        let mut parser = SseParser::new(stream);

        let event = parser.next().await.unwrap().unwrap();
        assert_eq!(event.event, Some("message".to_string()));
        assert_eq!(event.data, "hello");
    }

    #[tokio::test]
    async fn test_parse_with_id() {
        let data = b"id: 123\ndata: test\n\n";
        let stream = stream::once(async { Ok(Bytes::from(&data[..])) });
        let stream = Box::pin(stream);
        let mut parser = SseParser::new(stream);

        let event = parser.next().await.unwrap().unwrap();
        assert_eq!(event.id, Some("123".to_string()));
        assert_eq!(event.data, "test");
    }

    #[tokio::test]
    async fn test_ignore_comments() {
        let data = b": this is a comment\ndata: actual data\n\n";
        let stream = stream::once(async { Ok(Bytes::from(&data[..])) });
        let stream = Box::pin(stream);
        let mut parser = SseParser::new(stream);

        let event = parser.next().await.unwrap().unwrap();
        assert_eq!(event.data, "actual data");
    }

    #[tokio::test]
    async fn test_handle_split_chunks() {
        let chunk1 = b"data: par";
        let chunk2 = b"tial\ndata: data\n\n";
        let stream = stream::iter(vec![
            Ok(Bytes::from(&chunk1[..])),
            Ok(Bytes::from(&chunk2[..])),
        ]);
        let stream = Box::pin(stream);
        let mut parser = SseParser::new(stream);

        let event = parser.next().await.unwrap().unwrap();
        assert_eq!(event.data, "partial\ndata");
    }
}
