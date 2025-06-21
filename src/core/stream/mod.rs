pub mod codec;
pub mod compress;
pub mod crypto;
pub mod fallback;
pub mod fragment;
pub mod handshake;
pub mod virio;

use std::{pin::Pin, sync::Arc, task::Poll};

use parking_lot::Mutex;

use crate::{
    config::{Compress, Crypto},
    error,
};

use self::{compress::CompressedStream, crypto::EncryptedStream};

use super::{
    future::LazyFuture,
    io::{AsyncRead, AsyncWrite},
    AbstractStream,
};

pub trait UseCrypto<'a>: AsyncRead + AsyncWrite + Send + Unpin {
    fn use_crypto<'crypto, C>(self, cryptos: C) -> EncryptedStream<'a>
    where
        Self: Sized + 'a,
        C: Iterator<Item = &'crypto Crypto>,
    {
        crypto::encrypt_stream(self, cryptos)
    }
}

pub trait UseCompress<'a>: AsyncRead + AsyncWrite + Send + Unpin {
    fn use_compress<'compress, C>(self, compress: C) -> CompressedStream<'a>
    where
        C: Iterator<Item = &'compress Compress>,
        Self: Sized + 'a,
    {
        compress::compress_stream(self, compress)
    }
}

impl<'a, T> UseCrypto<'a> for T where T: AsyncRead + AsyncWrite + Send + Unpin {}
impl<'a, T> UseCompress<'a> for T where T: AsyncRead + AsyncWrite + Send + Unpin {}

#[derive(Clone)]
pub struct CloneableStream(Arc<Mutex<AbstractStream<'static>>>);

pub struct KeepAliveStream {
    stream: CloneableStream,
    poller: LazyFuture<'static, error::Result<()>>,
    last_check: std::time::Instant,
    last_write: std::time::Instant,
}

pub struct Socks5UdpStream<'a> {
    conn: AbstractStream<'a>,
    stream: AbstractStream<'a>,
}

impl CloneableStream {
    pub fn new<S>(s: S) -> Self
    where
        S: Into<AbstractStream<'static>>,
    {
        Self(Arc::new(Mutex::new(s.into())))
    }
}

impl<'a> Socks5UdpStream<'a> {
    pub fn new(dep: AbstractStream<'a>, stream: AbstractStream<'a>) -> Self {
        Self { conn: dep, stream }
    }
}

impl AsyncRead for CloneableStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        let mut this = self.0.lock();
        Pin::new(&mut *this).poll_read(cx, buf)
    }
}

impl AsyncWrite for CloneableStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        let mut this = self.0.lock();
        Pin::new(&mut *this).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<()>> {
        let mut this = self.0.lock();
        Pin::new(&mut *this).poll_flush(cx)
    }
}

impl KeepAliveStream {
    pub fn new(s: CloneableStream) -> Self {
        Self {
            stream: s,
            poller: LazyFuture::new(),
            last_check: std::time::Instant::now(),
            last_write: std::time::Instant::now(),
        }
    }

    /// 返回下一次更新时间
    pub fn next_update(&self) -> usize {
        unimplemented!()
    }

    pub fn duplicate(&self) -> AbstractStream<'static> {
        AbstractStream::new(self.stream.clone())
    }

    pub fn poll_check(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<error::Result<()>> {
        let stream = self.stream.clone();

        let last_write = self.last_write;
        let last_check = self.last_check;

        self.poller.poll(cx, move || {
            let mut stream = stream.clone();
            async move {
                let now = std::time::Instant::now();

                // if now.duration_since(last_check).as_secs() > 100000 {
                //     log::debug!("timeout ...");
                //     return Err(error::FusoError::Timeout);
                // }

                // if now.duration_since(last_write).as_secs() > 10 {
                //     stream.send_packet(&Fragment::Ping.encode()?).await?;
                // }

                Ok(())
            }
        })
    }
}

impl<'a> AsyncRead for Socks5UdpStream<'a> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<error::Result<usize>> {
        if let std::task::Poll::Ready(0) = Pin::new(&mut self.conn).poll_read(cx, buf)? {
            Poll::Ready(Err(error::FusoError::UdpForwardTerm))
        } else {
            Pin::new(&mut self.stream).poll_read(cx, buf)
        }
    }
}

impl<'a> AsyncWrite for Socks5UdpStream<'a> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<error::Result<usize>> {
        let mut sb = [0u8; 1];
        if let std::task::Poll::Ready(0) = Pin::new(&mut self.conn).poll_read(cx, &mut sb)? {
            Poll::Ready(Err(error::FusoError::UdpForwardTerm))
        } else {
            Pin::new(&mut self.stream).poll_write(cx, buf)
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<error::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
}
