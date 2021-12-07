use std::{net::SocketAddr, pin::Pin};

use futures::{AsyncRead, AsyncWrite};
use smol::net::TcpStream;

use crate::{Buffer, Rollback, RollbackEx, UdpStream};

#[derive(Debug, Clone)]
pub struct SafeStream<Inner> {
    inner: Rollback<Inner, Buffer<u8>>,
}

pub trait SafeStreamEx<T> {
    fn as_safe_stream(self) -> SafeStream<T>;
}

impl SafeStreamEx<Self> for TcpStream {
    #[inline]
    fn as_safe_stream(self) -> SafeStream<Self> {
        SafeStream { inner: self.roll() }
    }
}

impl SafeStreamEx<Self> for UdpStream {
    #[inline]
    fn as_safe_stream(self) -> SafeStream<Self> {
        SafeStream { inner: self.roll() }
    }
}

impl SafeStream<TcpStream> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.peer_addr()
    }
}

impl SafeStream<UdpStream> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.peer_addr()
    }
}

impl<Inner> SafeStream<Inner>
where
    Inner: Send + Sync + 'static,
{
    #[inline]
    pub async fn begin(&self) -> crate::Result<()> {
        self.inner.begin().await
    }

    #[inline]
    pub async fn back(&self) -> crate::Result<()> {
        self.inner.back().await
    }

    #[inline]
    pub async fn release(&self) -> crate::Result<()>{
        self.inner.release().await
    }

}

impl<Inner> AsyncRead for SafeStream<Inner>
where
    Inner: Clone + AsyncRead + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<Inner> AsyncWrite for SafeStream<Inner>
where
    Inner: Clone + AsyncWrite + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    #[inline]
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}
