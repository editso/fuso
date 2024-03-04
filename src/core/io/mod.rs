use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::error;

pub trait AsyncRead {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>>;
}

pub trait AsyncWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>>;

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<error::Result<()>>;
}

pub trait AsyncReadExt: AsyncRead + Unpin {
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> Read<'a, Self>
    where
        Self: Sized,
    {
        Read { reader: self, buf }
    }
}

pub trait AsyncWriteExt: AsyncWrite + Unpin {
    fn write<'a>(&'a mut self, buf: &'a [u8]) -> Write<'a, Self>
    where
        Self: Sized,
    {
        Write { writer: self, buf }
    }

    fn flush<'a>(&'a mut self) -> Flush<'a, Self>
    where
        Self: Sized,
    {
        Flush { writer: self }
    }
}

#[pin_project::pin_project]
pub struct Read<'a, R> {
    reader: &'a mut R,
    #[pin]
    buf: &'a mut [u8],
}

#[pin_project::pin_project]
pub struct Write<'a, W> {
    writer: &'a mut W,
    #[pin]
    buf: &'a [u8],
}

#[pin_project::pin_project]
pub struct Flush<'a, W> {
    writer: &'a mut W,
}

impl<'a, R> std::future::Future for Read<'a, R>
where
    R: AsyncRead + Unpin,
{
    type Output = error::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        Pin::new(&mut **this.reader).poll_read(cx, &mut *this.buf)
    }
}

impl<'a, W> std::future::Future for Write<'a, W>
where
    W: AsyncWrite + Unpin,
{
    type Output = error::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        Pin::new(&mut **this.writer).poll_write(cx, *this.buf)
    }
}

impl<'a, W> std::future::Future for Flush<'a, W>
where
    W: AsyncWrite + Unpin,
{
    type Output = error::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.writer).poll_flush(cx)
    }
}

impl<T> AsyncWriteExt for T where T: AsyncWrite + Unpin {}

impl<T> AsyncReadExt for T where T: AsyncRead + Unpin {}

impl<T> AsyncWrite for &mut T
where
    T: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(&mut **self).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<error::Result<()>> {
        Pin::new(&mut **self).poll_flush(cx)
    }
}

impl<T> AsyncRead for &mut T
where
    T: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        Pin::new(&mut **self).poll_read(cx, buf)
    }
}
 