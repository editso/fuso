pub mod ext;
pub mod io;
pub mod join;
pub mod r#macro;
pub mod select;
pub mod sync;
pub mod time;

use std::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll},
};

use crate::NetSocket;

pub type BoxedFuture<'lifetime, T> = Pin<Box<dyn Future<Output = T> + 'lifetime>>;

#[cfg(feature = "fuso-rt-smol")]
mod smol_io {
    pub use smol::io::{AsyncRead, AsyncWrite};
}

#[cfg(feature = "fuso-rt-custom")]
mod io {
    pub use futures::io::{AsyncRead, AsyncWrite};
}

#[cfg(feature = "fuso-rt-tokio")]
pub struct ReadBuf<'a> {
    buf: tokio::io::ReadBuf<'a>,
}

#[cfg(any(feature = "fuso-rt-smol", feature = "fuso-rt-custom"))]
#[derive(Debug)]
pub struct ReadBuf<'a> {
    buf: &'a mut [u8],
    offset: usize,
}

pub trait AsyncRead {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<crate::Result<usize>>;
}

pub trait AsyncWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>>;

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<()>>;

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<()>>;
}

pub trait Stream: NetSocket + AsyncRead + AsyncWrite + Unpin {}

impl AsyncRead for Box<dyn Stream + Send> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<crate::Result<usize>> {
        Pin::new(&mut **self).poll_read(cx, buf)
    }
}

impl AsyncWrite for Box<dyn Stream + Send> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        Pin::new(&mut **self).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        Pin::new(&mut **self).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        Pin::new(&mut **self).poll_close(cx)
    }
}

impl<S> Stream for S where S: NetSocket + AsyncWrite + AsyncRead + Unpin {}

#[cfg(feature = "fuso-rt-tokio")]
impl<T> AsyncWrite for T
where
    T: tokio::io::AsyncWrite,
{
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        tokio::io::AsyncWrite::poll_write(self, cx, buf).map_err(Into::into)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        tokio::io::AsyncWrite::poll_flush(self, cx).map_err(Into::into)
    }

    #[inline]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        tokio::io::AsyncWrite::poll_shutdown(self, cx).map_err(Into::into)
    }
}

#[cfg(feature = "fuso-rt-tokio")]
impl<T> AsyncRead for T
where
    T: tokio::io::AsyncRead,
{
    #[inline]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<crate::Result<usize>> {
        match tokio::io::AsyncRead::poll_read(self, cx, &mut buf.buf) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
            Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.filled().len())),
        }
    }
}

#[cfg(any(feature = "fuso-rt-smol", feature = "fuso-rt-custom"))]
impl<T> AsyncWrite for T
where
    T: smol_io::AsyncWrite,
{
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        futures::AsyncWrite::poll_write(self, cx, buf).map_err(Into::into)
    }

    #[inline]
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        futures::AsyncWrite::poll_flush(self, cx).map_err(Into::into)
    }

    #[inline]
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<crate::Result<()>> {
        futures::AsyncWrite::poll_close(self, cx).map_err(Into::into)
    }
}

#[cfg(any(feature = "fuso-rt-smol", feature = "fuso-rt-custom"))]
impl<T> AsyncRead for T
where
    T: smol_io::AsyncRead,
{
    #[inline]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<crate::Result<usize>> {
        match futures::AsyncRead::poll_read(self, cx, &mut buf.buf[buf.offset..])? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(n) => {
                buf.advance(n);
                Poll::Ready(Ok(n))
            }
        }
    }
}

impl<'a> ReadBuf<'a> {
    #[cfg(feature = "fuso-rt-tokio")]
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf: tokio::io::ReadBuf::new(buf),
        }
    }

    #[cfg(any(feature = "fuso-rt-smol", feature = "fuso-rt-custom"))]
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    pub fn len(&self) -> usize {
        self.buf.capacity()
    }

    #[cfg(any(feature = "fuso-rt-smol", feature = "fuso-rt-custom"))]
    pub fn remaining(&self) -> usize {
        self.len() - self.offset
    }

    pub fn has_remaining(&self) -> bool {
        self.remaining() > 0
    }

    #[cfg(any(feature = "fuso-rt-smol", feature = "fuso-rt-custom"))]
    pub fn position(&self) -> usize {
        self.offset
    }

    #[cfg(feature = "fuso-rt-tokio")]
    pub fn position(&self) -> usize {
        self.buf.filled().len()
    }

    #[cfg(feature = "fuso-rt-tokio")]
    pub fn iter_mut(&mut self) -> &mut [u8] {
        self.buf.filled_mut()
    }

    #[cfg(any(feature = "fuso-rt-smol", feature = "fuso-rt-custom"))]
    pub fn iter_mut(&mut self) -> &mut [u8] {
        &mut self.buf[..self.offset]
    }

    #[cfg(any(feature = "fuso-rt-smol", feature = "fuso-rt-custom"))]
    pub fn advance(&mut self, n: usize) {
        debug_assert!(self.offset + n <= self.buf.len());
        self.offset += n;
    }

    #[cfg(any(feature = "fuso-rt-smol", feature = "fuso-rt-custom"))]
    pub fn initialize_unfilled(&mut self) -> &mut [u8] {
        &mut self.buf[self.offset..]
    }
}

#[cfg(any(feature = "fuso-rt-smol", feature = "fuso-rt-custom"))]
impl<'a> Deref for ReadBuf<'a> {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.buf
    }
}

#[cfg(feature = "fuso-rt-tokio")]
impl<'a> Deref for ReadBuf<'a> {
    type Target = tokio::io::ReadBuf<'a>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl<'a> DerefMut for ReadBuf<'a> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buf
    }
}
