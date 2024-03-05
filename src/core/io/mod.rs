use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{error, select};

use super::BoxedFuture;

use crate::core::split::SplitStream;

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

    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> WriteAll<'a, Self>
    where
        Self: Sized,
    {
        WriteAll {
            writer: self,
            buf,
            pos: 0,
        }
    }
}

pub trait StreamExt {
    fn transfer<'a, T>(self, to: T) -> BoxedFuture<'a, error::Result<()>>
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'a,
        Self: Sized + AsyncRead + AsyncWrite + Unpin + Send + 'a,
    {
        Box::pin(async move {
            let (reader_2, writer_2) = to.split();
            let (reader_1, writer_1) = self.split();
            select![reader_1.copy(writer_2), reader_2.copy(writer_1)]
        })
    }

    fn copy<'a, T>(mut self, mut to: T) -> BoxedFuture<'a, error::Result<()>>
    where
        T: AsyncWrite + Unpin + Send + 'a,
        Self: AsyncRead + Unpin + Send + Sized + 'a,
    {
        Box::pin(async move {
            let mut buf = [0u8; 2048];

            loop {
                let n = self.read(&mut buf).await?;

                if n == 0 {
                    break Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
                }

                to.write_all(&buf).await?;
            }
        })
    }
}

impl<T> StreamExt for T {}

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
pub struct WriteAll<'a, W> {
    writer: &'a mut W,
    #[pin]
    buf: &'a [u8],
    pos: usize,
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

impl<'a, W> std::future::Future for WriteAll<'a, W>
where
    W: AsyncWrite + Unpin,
{
    type Output = error::Result<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        while *this.pos < this.buf.len() {
            match Pin::new(&mut **this.writer).poll_write(cx, &this.buf[*this.pos..])? {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(0) => {
                    return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe).into()))
                }
                Poll::Ready(n) => {
                    *this.pos += n;
                }
            };
        }

        Poll::Ready(Ok(()))
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
