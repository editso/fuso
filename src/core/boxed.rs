use std::pin::Pin;

use crate::{Address, AsyncRead, AsyncWrite, NetSocket, Stream};

pub struct FusoStream(Box<dyn Stream + Send + 'static>);

impl FusoStream {
    pub fn new<T>(t: T) -> Self
    where
        T: Stream + Send + 'static,
    {
        Self(Box::new(t))
    }
}

pub trait ToBoxStream {
    fn into_boxed_stream(self) -> FusoStream;
}

unsafe impl Sync for FusoStream {}

impl NetSocket for FusoStream {
    fn peer_addr(&self) -> crate::Result<Address> {
        self.0.peer_addr()
    }

    fn local_addr(&self) -> crate::Result<Address> {
        self.0.local_addr()
    }
}

impl AsyncWrite for FusoStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        Pin::new(&mut *self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut *self.0).poll_flush(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut *self.0).poll_close(cx)
    }
}

impl AsyncRead for FusoStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        Pin::new(&mut *self.0).poll_read(cx, buf)
    }
}

impl<T> ToBoxStream for T
where
    T: NetSocket + Stream + Send + 'static,
{
    fn into_boxed_stream(self) -> FusoStream {
        FusoStream::new(self)
    }
}
