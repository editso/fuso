use std::{
    io::{Cursor, Write},
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::Poll,
};

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite};

use crate::{Result, SafeStream};

use smol::{future::FutureExt, io::AsyncWriteExt, net::TcpStream};

use crate::Buffer;

pub type DynCipher = dyn self::Cipher + Send + Sync + 'static;

#[derive(Clone)]
pub struct Crypt<T, C> {
    buf: Arc<Mutex<Buffer<u8>>>,
    target: Arc<Mutex<T>>,
    cipher: Arc<Mutex<C>>,
}

#[async_trait]
pub trait Advice<T, O> {
    async fn advice(&self, io: &mut T) -> Result<Option<O>>;
}

pub trait Security<T, O> {
    fn cipher(self, cipher: O) -> Crypt<T, O>;
}

#[async_trait]
pub trait SecurityEx<T, O>: Security<T, O>
where
    T: AsyncWrite + AsyncRead + Unpin + Send + Sync + 'static,
{
    async fn select_cipher(
        &mut self,
        advices: &[Arc<dyn Advice<T, O> + Send + Sync + 'static>],
    ) -> Result<Option<O>>;
}

#[async_trait]
impl<T> SecurityEx<T, Box<DynCipher>> for T
where
    T: AsyncWrite + AsyncRead + Unpin + Send + Sync + 'static,
{
    async fn select_cipher(
        &mut self,
        advices: &[Arc<dyn Advice<T, Box<DynCipher>> + Send + Sync + 'static>],
    ) -> Result<Option<Box<DynCipher>>> {
        for advice in advices.iter() {
            let cipher = advice.advice(self).await?;
            if let Some(cipher) = cipher {
                return Ok(Some(cipher));
            }
        }
        Ok(None)
    }
}

pub trait Cipher {
    fn poll_decrypt(
        &mut self,
        io: Box<&mut (dyn AsyncRead + Unpin + Send + Sync + 'static)>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<Vec<u8>>>;

    fn poll_encrypt(
        &mut self,
        io: Box<&mut (dyn AsyncWrite + Unpin + Send + Sync + 'static)>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>>;
}

#[derive(Clone)]
pub struct Xor {
    num: u8,
}

impl<C> Crypt<TcpStream, C> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.lock().unwrap().local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.lock().unwrap().peer_addr()
    }
}

impl<C> Crypt<SafeStream<TcpStream>, C> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.lock().unwrap().local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.lock().unwrap().peer_addr()
    }
}

#[async_trait]
impl<T, C> Security<T, C> for T
where
    T: AsyncWrite + AsyncRead + Unpin + Send + Sync + 'static,
    C: Cipher + Send + Sync + 'static,
{
    #[inline]
    fn cipher(self, c: C) -> Crypt<T, C> {
        Crypt {
            target: Arc::new(Mutex::new(self)),
            buf: Arc::new(Mutex::new(Buffer::new())),
            cipher: Arc::new(Mutex::new(c)),
        }
    }
}

impl<T> Security<T, Box<DynCipher>> for T
where
    T: AsyncWrite + AsyncRead + Send + Sync + 'static,
{
    #[inline]
    fn cipher(self, c: Box<DynCipher>) -> Crypt<T, Box<DynCipher>> {
        Crypt {
            target: Arc::new(Mutex::new(self)),
            buf: Arc::new(Mutex::new(Buffer::new())),
            cipher: Arc::new(Mutex::new(c)),
        }
    }
}

#[async_trait]
impl<T, C> AsyncRead for Crypt<T, C>
where
    T: AsyncRead + Unpin + Send + Sync + 'static,
    C: Cipher + Unpin + Send + Sync + 'static,
{
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let io_buf = self.buf.clone();

        let mut io_buf = io_buf.lock().unwrap();

        if !io_buf.is_empty() {
            Pin::new(&mut *io_buf).poll_read(cx, buf)
        } else {
            let mut core = self.target.lock().unwrap();
            let mut cipher = self.cipher.lock().unwrap();
            let io = Box::new(&mut *core as _);

            match Pin::new(&mut *cipher).poll_decrypt(io, cx, buf) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Ready(Ok(data)) => {
                    let total = buf.len();
                    let mut cur = Cursor::new(buf);

                    let write_len = if total >= data.len() {
                        cur.write_all(&data).unwrap();
                        data.len()
                    } else {
                        cur.write_all(&data[..total]).unwrap();
                        io_buf.push_back(&data[total..]);
                        total
                    };

                    Poll::Ready(Ok(write_len))
                }
            }
        }
    }
}

#[async_trait]
impl<T, C> AsyncWrite for Crypt<T, C>
where
    T: AsyncWrite + Unpin + Send + Sync + 'static,
    C: Cipher + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut core = self.target.lock().unwrap();
        let mut cipher = self.cipher.lock().unwrap();
        let io = Box::new(&mut *core as _);

        Pin::new(&mut *cipher).poll_encrypt(io, cx, buf)
    }

    #[inline]
    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let mut core = self.target.lock().unwrap();
        Pin::new(&mut *core).poll_flush(cx)
    }

    #[inline]
    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let mut core = self.target.lock().unwrap();
        Pin::new(&mut *core).poll_close(cx)
    }
}

#[async_trait]
impl<T> AsyncRead for Crypt<T, Box<DynCipher>>
where
    T: AsyncRead + Unpin + Send + Sync + 'static,
{
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let io_buf = self.buf.clone();

        let mut io_buf = io_buf.lock().unwrap();

        if !io_buf.is_empty() {
            Pin::new(&mut *io_buf).poll_read(cx, buf)
        } else {
            let mut core = self.target.lock().unwrap();
            let mut cipher = self.cipher.lock().unwrap();
            let io = Box::new(&mut *core as _);
            match Pin::new(&mut *cipher).poll_decrypt(io, cx, buf) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Ready(Ok(data)) => {
                    let total = buf.len();
                    let mut cur = Cursor::new(buf);
                    
                    let write_len = if total >= data.len() {
                        cur.write_all(&data).unwrap();
                        data.len()
                    } else {
                        cur.write_all(&data[..total]).unwrap();
                        io_buf.push_back(&data[total..]);
                        total
                    };

                    Poll::Ready(Ok(write_len))
                }
            }
        }
    }
}

#[async_trait]
impl<T> AsyncWrite for Crypt<T, Box<DynCipher>>
where
    T: AsyncWrite + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut core = self.target.lock().unwrap();
        let mut cipher = self.cipher.lock().unwrap();
        let io = Box::new(&mut *core as _);
        Pin::new(&mut *cipher).poll_encrypt(io, cx, buf)
    }

    #[inline]
    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let mut core = self.target.lock().unwrap();
        Pin::new(&mut *core).poll_flush(cx)
    }

    #[inline]
    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let mut core = self.target.lock().unwrap();
        Pin::new(&mut *core).poll_close(cx)
    }
}

impl Xor {
    #[inline]
    pub fn new(num: u8) -> Self {
        Self { num }
    }
}

impl Cipher for Xor {
    fn poll_decrypt(
        &mut self,
        mut io: Box<&mut (dyn AsyncRead + Unpin + Send + Sync + 'static)>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<Vec<u8>>> {
        match Pin::new(&mut io).poll_read(cx, buf)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(n) => Poll::Ready(Ok(buf[..n]
                .iter()
                .map(|n| self.num ^ n)
                .collect::<Vec<u8>>())),
        }
    }

    fn poll_encrypt(
        &mut self,
        mut io: Box<&mut (dyn AsyncWrite + Unpin + Send + Sync + 'static)>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
      
        match Pin::new(&mut io)
            .write_all(&buf.into_iter().map(|n| self.num ^ n).collect::<Vec<u8>>())
            .poll(cx)?
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(_) => Poll::Ready(Ok(buf.len())),
        }
    }
}
