use std::{future::Future, ops::Deref, pin::Pin, sync::Arc, task::Poll};

use crate::{
    ext::{AsyncReadExt, AsyncWriteExt},
    AsyncRead, AsyncWrite, NetSocket,
};

type BoxedFuture = Pin<Box<dyn Future<Output = crate::Result<()>> + Send + 'static>>;

macro_rules! unwrap {
    ($r: expr) => {
        match $r {
            Ok(r) => r,
            Err(e) => return Poll::Ready(Err(e.into())),
        }
    };
}

pub struct Forward {
    futures: Vec<BoxedFuture>,
}

pub struct Inner<S>(std::sync::Mutex<S>);

pub struct ReadHalf<R>(Arc<Inner<R>>);

pub struct WriteHalf<W>(Arc<Inner<W>>);

impl<S> NetSocket for Inner<S>
where
    S: NetSocket,
{
    fn peer_addr(&self) -> crate::Result<crate::Address> {
        self.0.lock()?.peer_addr()
    }

    fn local_addr(&self) -> crate::Result<crate::Address> {
        self.0.lock()?.local_addr()
    }
}

impl<R> NetSocket for ReadHalf<R>
where
    R: NetSocket,
{
    fn peer_addr(&self) -> crate::Result<crate::Address> {
        self.0.peer_addr()
    }
    fn local_addr(&self) -> crate::Result<crate::Address> {
        self.0.local_addr()
    }
}

impl<W> NetSocket for WriteHalf<W>
where
    W: NetSocket,
{
    fn peer_addr(&self) -> crate::Result<crate::Address> {
        self.0.peer_addr()
    }

    fn local_addr(&self) -> crate::Result<crate::Address> {
        self.0.local_addr()
    }
}

impl Future for Forward {
    type Output = crate::Result<()>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut futures = Vec::new();
        while let Some(mut future) = self.futures.pop() {
            match Pin::new(&mut future).poll(cx) {
                Poll::Pending => futures.push(future),
                Poll::Ready(r) => {
                    return Poll::Ready(r);
                }
            }
        }

        drop(std::mem::replace(&mut self.futures, futures));

        if self.futures.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

pub fn forward<S1, S2>(s1: S1, s2: S2) -> Forward
where
    S1: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    S2: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (s1_reader, s1_writer) = split(s1);
    let (s2_reader, s2_writer) = split(s2);

    fn copy<R, W>(mut reader: R, mut writer: W) -> BoxedFuture
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Box::pin(async move {
            let mut buf = unsafe {
                let mut buf = Vec::with_capacity(1500);
                buf.set_len(1500);
                buf
            };

            loop {
                let r = reader.read(&mut buf).await;

                if r.is_err() {
                    return Err(unsafe { r.unwrap_err_unchecked() });
                }

                let n = unsafe { r.unwrap_unchecked() };

                if n == 0 {
                    let _ = writer.flush().await;
                    return writer.close().await;
                }

                log::trace!("forward {}bytes data", n);

                let r = writer.write_all(&buf[..n]).await;

                if r.is_err() {
                    return Err(unsafe {
                        let err = r.unwrap_err_unchecked();
                        log::trace!("forward error {}", err);
                        err
                    });
                }
            }
        })
    }

    Forward {
        futures: vec![copy(s1_reader, s2_writer), copy(s2_reader, s1_writer)],
    }
}

impl<T> Deref for Inner<T> {
    type Target = std::sync::Mutex<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<R> AsyncRead for ReadHalf<R>
where
    R: AsyncRead + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> Poll<crate::Result<usize>> {
        let mut inner = unwrap!(self.0.lock());
        Pin::new(&mut *inner).poll_read(cx, buf)
    }
}

impl<W> AsyncWrite for WriteHalf<W>
where
    W: AsyncWrite + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        let mut inner = unwrap!(self.0.lock());
        Pin::new(&mut *inner).poll_write(cx, buf)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<()>> {
        let mut inner = unwrap!(self.0.lock());
        Pin::new(&mut *inner).poll_close(cx)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<()>> {
        let mut inner = unwrap!(self.0.lock());
        Pin::new(&mut *inner).poll_flush(cx)
    }
}

pub fn split<T>(t: T) -> (ReadHalf<T>, WriteHalf<T>)
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let inner = Arc::new(Inner(std::sync::Mutex::new(t)));

    (ReadHalf(inner.clone()), WriteHalf(inner))
}

impl<T> Clone for ReadHalf<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Clone for WriteHalf<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
