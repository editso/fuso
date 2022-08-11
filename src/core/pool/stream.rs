use std::{io, pin::Pin, task::Poll};

use crate::{AsyncRead, AsyncWrite};

macro_rules! reset {
    () => {
        io::Error::from(io::ErrorKind::ConnectionReset).into()
    };
}

pub struct PoolStream<S> {
    unique: u64,
    stream: Option<S>,
    retreat_fn: Option<Box<dyn FnOnce(S, u64) + Send + 'static>>,
}

unsafe impl<S> Send for PoolStream<S> {}
unsafe impl<S> Sync for PoolStream<S> {}

impl<S> AsyncRead for PoolStream<S>
where
    S: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        match self.stream.as_mut() {
            None => Poll::Ready(Err(reset!())),
            Some(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl<S> AsyncWrite for PoolStream<S>
where
    S: AsyncWrite + Unpin,
{
    fn poll_close(
        mut self: Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<()>> {
        match self.stream.take() {
            None => Poll::Ready(Err(reset!())),
            Some(stream) => match self.retreat_fn.take() {
                None => Poll::Ready(Err(reset!())),
                Some(retreat_fn) => {
                    retreat_fn(stream, self.unique);
                    Poll::Ready(Ok(()))
                }
            },
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<()>> {
        match self.stream.as_mut() {
            None => Poll::Ready(Err(reset!())),
            Some(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        match self.stream.as_mut() {
            None => Poll::Ready(Err(reset!())),
            Some(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }
}

impl<S> Drop for PoolStream<S> {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            if let Some(retreat_fn) = self.retreat_fn.take() {
                retreat_fn(stream, self.unique)
            }
        }
    }
}
