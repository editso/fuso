use std::{
    io::{Cursor, Read, Write},
    pin::Pin,
    task::Poll,
};

use crate::core::io::{AsyncRead, AsyncWrite};

pub struct Fallback<S> {
    inner: S,
    outed: Option<Cursor<Vec<u8>>>,
    cached: Option<Cursor<Vec<u8>>>,
}

impl<S> Fallback<S> {
    pub fn mark(&mut self) {
        if self.cached.is_none() {
            self.cached.replace(Default::default());
        }
    }

    pub fn reset(&mut self) {
        if let Some(cached) = self.cached.take() {
            match self.outed.as_mut() {
                Some(outed) => {
                    let _ = outed.write_all(&cached.into_inner());
                }
                None => {
                    self.outed.replace(cached);
                }
            }
        }
    }

    pub fn into_inner(mut self) -> (Option<Vec<u8>>, S) {
        self.reset();
        match self.outed {
            None => (None, self.inner),
            Some(outed) => (Some(outed.into_inner()), self.inner),
        }
    }
}

impl<S> From<S> for Fallback<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn from(value: S) -> Self {
        Self {
            inner: value,
            outed: Default::default(),
            cached: Default::default(),
        }
    }
}

impl<S> AsyncRead for Fallback<S>
where
    S: AsyncRead + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        if let Some(outed) = self.outed.as_mut() {
            let n = outed.read(buf)?;
            if n > 0 {
                return Poll::Ready(Ok(n));
            }
        }

        match Pin::new(&mut self.inner).poll_read(cx, buf)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(n) => {
                if let Some(cached) = self.cached.as_mut() {
                    cached.write_all(&buf[..n])?;
                }
                Poll::Ready(Ok(n))
            }
        }
    }
}

impl<S> AsyncWrite for Fallback<S>
where
    S: AsyncWrite + Unpin,
{
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }
}
