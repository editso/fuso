use std::future::Future;
use std::io::Write;
use std::task::Poll;
use std::{pin::Pin, task::Context};

use crate::r#async::{AsyncRead, AsyncWrite};

pub struct Fallback<T> {
    target: T,
    strict: bool,
    rest_buf: Option<std::io::Cursor<Vec<u8>>>,
    begin_buf: Option<std::io::Cursor<Vec<u8>>>,
}

pub struct Mark<'a, IO>(&'a mut Fallback<IO>);

pub struct Backward<'a, IO>(&'a mut Fallback<IO>);

impl<T> Fallback<T> {
    #[inline]
    pub fn new(t: T, strict: bool) -> Self {
        Fallback {
            target: t,
            strict: strict,
            rest_buf: Default::default(),
            begin_buf: Default::default(),
        }
    }

    pub fn with_strict(t: T) -> Self {
        Fallback {
            target: t,
            strict: true,
            rest_buf: Default::default(),
            begin_buf: Default::default(),
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

    pub fn back_data(&self) -> Option<&Vec<u8>> {
        self.begin_buf.as_ref().map(|buf| buf.get_ref())
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
        let poll = loop {
            if let Some(reset_buf) = self.rest_buf.as_mut() {
                let rem = buf.remaining();
                let pos = buf.position();

                if rem != 0 {
                    let n = std::io::Read::read(reset_buf, &mut buf.iter_mut()[pos..])?;
                    buf.advance(n);
                    if buf.remaining() == 0 {
                        break Poll::Ready(Ok(n));
                    } else {
                        drop(std::mem::replace(&mut self.rest_buf, Default::default()))
                    }
                }
            } else {
                break Pin::new(&mut self.target).poll_read(cx, buf);
            }
        }?;

        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready(n) => {
                if let Some(begin) = self.begin_buf.as_mut() {
                    let pos = begin.position();
                    begin.write_all(&buf.iter_mut()[..n])?;
                    begin.set_position(pos);
                }
                Poll::Ready(Ok(n))
            }
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
        if self.strict && (self.rest_buf.is_some() || self.begin_buf.is_some()) {

            log::warn!("write data in strict mode while buffer data has not been processed");

            let mut buffers = Vec::new();

            if let Some(buf) = self.begin_buf.take() {
                buffers.push(buf.into_inner())
            }

            if let Some(buf) = self.rest_buf.take() {
                buffers.push(buf.into_inner())
            }

            return Poll::Ready(Err(crate::error::Kind::Fallback(buffers).into()));
        } else {

            drop(self.begin_buf.take());
            drop(self.rest_buf.take());
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
        if let Some(mut buf) = this.begin_buf.take() {
            let pos = buf.position();
            this.rest_buf
                .take()
                .map(|last| buf.write_all(&last.into_inner()));
            buf.set_position(pos);
            drop(std::mem::replace(&mut this.rest_buf, Some(buf)));
        }

        drop(std::mem::replace(
            &mut this.begin_buf,
            Some(Default::default()),
        ));

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
        if let Some(mut buf) = this.begin_buf.take() {
            let pos = buf.position();

            this.rest_buf
                .take()
                .map(|last| buf.write_all(&last.into_inner()));

            buf.set_position(pos);

            drop(std::mem::replace(&mut this.rest_buf, Some(buf)));
        }

        Poll::Ready(Ok(()))
    }
}
