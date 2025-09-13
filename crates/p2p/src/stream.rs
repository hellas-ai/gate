//! Stream utilities for P2P connections

use iroh::endpoint::{RecvStream, SendStream};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};

/// Wrapper that combines Iroh's RecvStream and SendStream into a single
/// type that implements AsyncRead + AsyncWrite
pub struct CombinedStream {
    recv: RecvStream,
    send: SendStream,
}

impl CombinedStream {
    /// Create a new combined stream from separate recv and send streams
    pub fn new(recv: RecvStream, send: SendStream) -> Self {
        Self { recv, send }
    }

    /// Split the combined stream back into separate recv and send streams
    pub fn split(self) -> (RecvStream, SendStream) {
        (self.recv, self.send)
    }

    /// Get a reference to the receive stream
    pub fn recv(&self) -> &RecvStream {
        &self.recv
    }

    /// Get a mutable reference to the receive stream
    pub fn recv_mut(&mut self) -> &mut RecvStream {
        &mut self.recv
    }

    /// Get a reference to the send stream
    pub fn send(&self) -> &SendStream {
        &self.send
    }

    /// Get a mutable reference to the send stream
    pub fn send_mut(&mut self) -> &mut SendStream {
        &mut self.send
    }
}

impl AsyncRead for CombinedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match Pin::new(&mut self.recv).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(io::Error::other(format!("Read error: {e}")))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for CombinedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.send).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => Poll::Ready(Ok(n)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(io::Error::other(format!("Write error: {e}")))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match Pin::new(&mut self.send).poll_flush(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(io::Error::other(format!("Flush error: {e}")))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Iroh's SendStream doesn't have poll_finish, so we'll just report success
        // The actual shutdown happens when the stream is dropped
        Poll::Ready(Ok(()))
    }
}

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Operation timed out")]
    Timeout,

    #[error("Message too large: {0} > {1}")]
    MessageTooLarge(usize, usize),
}

type Result<T> = std::result::Result<T, StreamError>;

/// Utilities for working with P2P streams
pub struct StreamUtils;

impl StreamUtils {
    /// Copy data bidirectionally between two streams
    pub async fn copy_bidirectional<A, B>(stream_a: &mut A, stream_b: &mut B) -> Result<(u64, u64)>
    where
        A: AsyncRead + AsyncWrite + Unpin,
        B: AsyncRead + AsyncWrite + Unpin,
    {
        Ok(tokio::io::copy_bidirectional(stream_a, stream_b).await?)
    }
}
