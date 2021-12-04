use std::{net::SocketAddr, pin::Pin, sync::Arc};

use futures::{AsyncRead, AsyncWrite};
use smol::net::TcpStream;

use crate::{Buffer, Rollback, RollbackEx, UdpStream};

#[derive(Debug, Clone)]
pub struct FusoStream<Inner> {
    inner: Arc<std::sync::Mutex<Rollback<Inner, Buffer<u8>>>>,
}

pub trait FusoStreamEx<T> {
    fn as_fuso_stream(self) -> FusoStream<T>;
}

impl FusoStreamEx<Self> for TcpStream {
    #[inline]
    fn as_fuso_stream(self) -> FusoStream<Self> {
        FusoStream {
            inner: Arc::new(std::sync::Mutex::new(self.roll())),
        }
    }
}

impl FusoStreamEx<Self> for UdpStream {
    #[inline]
    fn as_fuso_stream(self) -> FusoStream<Self> {
        FusoStream {
            inner: Arc::new(std::sync::Mutex::new(self.roll())),
        }
    }
}

impl FusoStream<TcpStream> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.lock().unwrap().local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.lock().unwrap().peer_addr()
    }
}

impl FusoStream<UdpStream> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.lock().unwrap().local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.lock().unwrap().peer_addr()
    }
}

impl<Inner> FusoStream<Inner> {
    #[inline]
    pub async fn begin(&self) -> crate::Result<()> {
        self.inner.lock().unwrap().begin().await
    }

    #[inline]
    pub async fn back(&self) -> crate::Result<()> {
        self.inner.lock().unwrap().back().await
    }
}

impl<Inner> AsyncRead for FusoStream<Inner>
where
    Inner: AsyncRead + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut io = self.inner.lock().unwrap();
        Pin::new(&mut *io).poll_read(cx, buf)
    }
}


impl<Inner> AsyncWrite for FusoStream<Inner>
where
    Inner: AsyncWrite + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut io = self.inner.lock().unwrap();
        Pin::new(&mut *io).poll_write(cx, buf)
    }

    #[inline]
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let mut io = self.inner.lock().unwrap();
        Pin::new(&mut *io).poll_flush(cx)
    }

    #[inline]
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let mut io = self.inner.lock().unwrap();
        Pin::new(&mut *io).poll_close(cx)
    }
}
