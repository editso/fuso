pub mod ext;

use std::io::Write;
use std::ops::{Deref, DerefMut};
use std::task::Poll;
use std::{pin::Pin, task::Context};

use crate::r#async::{AsyncRead, AsyncWrite};

use crate::Result;

pub struct Fallback<T> {
    io: T,
    rest_buf: Option<std::io::Cursor<Vec<u8>>>,
    begin_buf: Option<std::io::Cursor<Vec<u8>>>,
}

impl<T> Fallback<T> {
    pub fn new(t: T) -> Self {
        Fallback {
            io: t,
            rest_buf: Default::default(),
            begin_buf: Default::default(),
        }
    }
}

pub trait StreamGuard<T> {
    fn into_inner(self) -> T;
    fn poll_ward(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>>;
    fn poll_backward(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<()>>;
}

// impl<T> Deref for Fallback<T> {
//     type Target = T;

//     fn deref(&self) -> &Self::Target {
//         &self.io
//     }
// }

// impl<T> DerefMut for Fallback<T> {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.io
//     }
// }

impl<T> StreamGuard<T> for Fallback<T>
where
    T: AsyncWrite + AsyncWrite + Unpin,
{
    fn into_inner(self) -> T {
        self.io
    }

    fn poll_ward(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<()>> {
        if let Some(mut buf) = self.begin_buf.take() {
            let pos = buf.position();
            self.rest_buf
                .take()
                .map(|last| buf.write_all(&last.into_inner()));
            buf.set_position(pos);
            drop(std::mem::replace(&mut self.rest_buf, Some(buf)));
        }

        drop(std::mem::replace(
            &mut self.begin_buf,
            Some(Default::default()),
        ));

        Poll::Ready(Ok(()))
    }

    fn poll_backward(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<()>> {
        if let Some(mut buf) = self.begin_buf.take() {
            let pos = buf.position();

            self.rest_buf
                .take()
                .map(|last| buf.write_all(&last.into_inner()));

            buf.set_position(pos);

            drop(std::mem::replace(&mut self.rest_buf, Some(buf)));
        }

        Poll::Ready(Ok(()))
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
                break Pin::new(&mut self.io).poll_read(cx, buf);
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
        if self.begin_buf.is_some() {
            drop(std::mem::replace(&mut self.begin_buf, Default::default()));
        }

        if self.rest_buf.is_some() {
            drop(std::mem::replace(&mut self.rest_buf, Default::default()));
        }

        Pin::new(&mut self.io).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut self.io).poll_close(cx)
    }
}
