use std::future::Future;

use std::task::Poll;
use std::{pin::Pin, task::Context};

use crate::r#async::{AsyncRead, AsyncWrite};
use crate::{Kind, NetSocket, Stream};

use super::buffer::Buffer;

pub struct Fallback<T> {
    target: T,
    strict: bool,
    backed_buf: Option<Buffer<u8>>,
    marked_buf: Option<Buffer<u8>>,
}

pub struct Mark<'a, IO>(&'a mut Fallback<IO>);

pub struct Backward<'a, IO>(&'a mut Fallback<IO>);

impl<T> Fallback<T> {
    #[inline]
    pub fn new(t: T, strict: bool) -> Self {
        Fallback {
            target: t,
            strict: strict,
            backed_buf: Default::default(),
            marked_buf: Default::default(),
        }
    }

    pub fn with_strict(t: T) -> Self {
        Fallback {
            target: t,
            strict: true,
            backed_buf: Default::default(),
            marked_buf: Default::default(),
        }
    }
}

impl<T> Fallback<T> {
    #[inline]
    pub fn into_inner(self) -> T {
        self.target
    }

    #[inline]
    pub fn mark<'a>(&'a mut self) -> Mark<'a, T>
    where
        Self: Sized + Unpin,
    {
        Mark(self)
    }

    #[inline]
    pub fn backward<'a>(&'a mut self) -> Backward<'a, T>
    where
        Self: Sized + Unpin,
    {
        Backward(self)
    }

    pub fn consume_back_data(&mut self) {
        self.marked_buf.take();
        self.backed_buf.take();
    }

    pub fn back_data(&mut self) -> Option<Vec<u8>> {
        self.backed_buf.take().map(|mut buf| {
            let mut backed = Vec::with_capacity(buf.len());
            unsafe {
                backed.set_len(buf.len());
            }

            let _ = buf.read_to_buffer(&mut backed);

            backed
        })
    }
}

impl<T> NetSocket for Fallback<T>
where
    T: Stream,
{
    fn peer_addr(&self) -> crate::Result<crate::Address> {
        self.target.peer_addr()
    }

    fn local_addr(&self) -> crate::Result<crate::Address> {
        self.target.local_addr()
    }
}

impl<T> AsyncRead for Fallback<T>
where
    T: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        let poll = {
            match self.backed_buf.take() {
                None => Pin::new(&mut self.target).poll_read(cx, buf),
                Some(mut backed) => {
                    let n = backed.read_to_buffer(buf.initialize_unfilled());
                    buf.advance(n);
                    if !backed.is_empty() {
                        drop(std::mem::replace(&mut self.backed_buf, Some(backed)));
                    }
                    Poll::Ready(Ok(n))
                }
            }
        };

        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Ready(Ok(n)) => match self.marked_buf.as_mut() {
                None => Poll::Ready(Ok(n)),
                Some(marked) => {
                    marked.push_back(buf.iter_mut());
                    Poll::Ready(Ok(n))
                }
            },
        }
    }
}

impl<T> AsyncWrite for Fallback<T>
where
    T: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        if self.strict && (self.backed_buf.is_some() || self.marked_buf.is_some()) {
            log::warn!("write data in strict mode while buffer data has not been processed");
            return Poll::Ready(Err(Kind::Fallback.into()));
        } else {
            drop(self.marked_buf.take());
            drop(self.backed_buf.take());
        }

        Pin::new(&mut self.target).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut self.target).poll_flush(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut self.target).poll_close(cx)
    }
}

impl<'a, IO> Future for Mark<'a, IO>
where
    IO: Unpin,
{
    type Output = crate::Result<()>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = &mut self.0;

        if !this.marked_buf.is_some() {
            drop(std::mem::replace(&mut this.marked_buf, Some(Buffer::new())));
        }

        Poll::Ready(Ok(()))
    }
}

impl<'a, IO> Future for Backward<'a, IO>
where
    IO: Unpin,
{
    type Output = crate::Result<()>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = &mut self.0;
        if let Some(mut marked) = this.marked_buf.take() {
            let backed_buf = if let Some(mut backed) = this.backed_buf.take() {
                let mut buf = Vec::with_capacity(marked.len());
                unsafe{
                    buf.set_len(marked.len());
                }
                marked.read_to_buffer(&mut buf);
                backed.push_all(buf);
                backed
            }else{
                marked
            };
            drop(std::mem::replace(&mut this.backed_buf, Some(backed_buf)));
        }

        Poll::Ready(Ok(()))
    }
}
