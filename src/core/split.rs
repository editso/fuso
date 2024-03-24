use std::{pin::Pin, sync::Arc};

use parking_lot::Mutex;

use super::io::{AsyncRead, AsyncWrite};

pub struct ReadHalf<S> {
    reader: Arc<Mutex<S>>,
}

pub struct WriteHalf<S> {
    writer: Arc<Mutex<S>>,
}

pub trait SplitStream: AsyncWrite + AsyncRead {
    fn split(self) -> (ReadHalf<Self>, WriteHalf<Self>)
    where
        Self: Sized,
    {
        let stream = Arc::new(Mutex::new(self));

        (
            ReadHalf {
                reader: stream.clone(),
            },
            WriteHalf { writer: stream },
        )
    }
}

impl<T> SplitStream for T where T: AsyncWrite + AsyncRead + Unpin {}


impl<S> AsyncRead for ReadHalf<S>
where
    S: AsyncRead + Unpin,
{
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        let mut reader = self.reader.lock();
        Pin::new(&mut *reader).poll_read(cx, buf)
    }
}

impl<S> AsyncWrite for WriteHalf<S>
where
    S: AsyncWrite + Unpin,
{
    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<()>> {
        let mut reader = self.writer.lock();
        Pin::new(&mut *reader).poll_flush(cx)
    }

    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        let mut reader = self.writer.lock();
        Pin::new(&mut *reader).poll_write(cx, buf)
    }
}

impl<S> Clone for ReadHalf<S> {
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
        }
    }
}

impl<S> Clone for WriteHalf<S> {
    fn clone(&self) -> Self {
        Self {
            writer: self.writer.clone(),
        }
    }
}
