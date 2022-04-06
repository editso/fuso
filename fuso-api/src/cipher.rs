use std::{
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::Poll,
};

use async_trait::async_trait;
use futures::{AsyncRead, AsyncReadExt, AsyncWrite, Future};

use crate::{Result, SafeStream};

use smol::{future::FutureExt, io::AsyncWriteExt, net::TcpStream};

use crate::Buffer;

pub type DynCipher = dyn self::Cipher + Send + Sync;

#[derive(Clone)]
pub struct Crypt<T, C> {
    core: T,
    cipher: C,
    store: Buffer<u8>,
    decrypt_fut: Arc<Mutex<Option<Pin<Box<dyn Future<Output = std::io::Result<Vec<u8>>> + Send>>>>>,
    encrypt_fut: Arc<Mutex<Option<Pin<Box<dyn Future<Output = std::io::Result<usize>> + Send>>>>>,
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
    fn decrypt(
        &self,
        io: Pin<Box<dyn AsyncRead + Unpin + Send>>,
        max_buf: usize,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<Vec<u8>>> + Send>>;

    fn encrypt(
        &self,
        io: Pin<Box<dyn AsyncWrite + Unpin + Send>>,
        buf: &[u8],
    ) -> Pin<Box<dyn Future<Output = std::io::Result<usize>> + Send>>;
}

#[derive(Clone)]
pub struct Xor {
    num: u8,
}

impl<C> Crypt<TcpStream, C> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.core.local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.core.peer_addr()
    }
}

impl<C> Crypt<SafeStream<TcpStream>, C> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.core.local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.core.peer_addr()
    }
}

#[async_trait]
impl<'a, T, C> Security<T, C> for T
where
    T: AsyncWrite + AsyncRead + Unpin + Send + Sync + 'static,
    C: Cipher + Send + Sync + 'static,
{
    #[inline]
    fn cipher(self, c: C) -> Crypt<T, C> {
        Crypt {
            core: self,
            store: Buffer::new(),
            cipher: c,
            decrypt_fut: Arc::new(Mutex::new(None)),
            encrypt_fut: Arc::new(Mutex::new(None)),
        }
    }
}

impl<'a, T> Security<T, Box<DynCipher>> for T
where
    T: AsyncWrite + AsyncRead + Send + Sync + 'static,
{
    #[inline]
    fn cipher(self, c: Box<DynCipher>) -> Crypt<T, Box<DynCipher>> {
        Crypt {
            core: self,
            store: Buffer::new(),
            cipher: c,
            decrypt_fut: Arc::new(Mutex::new(None)),
            encrypt_fut: Arc::new(Mutex::new(None)),
        }
    }
}

impl<T> AsyncRead for Crypt<T, Box<DynCipher>>
where
    T: AsyncRead + Clone + Unpin + Send + Sync + 'static,
{
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut store = self.store.clone();

        if !store.is_empty() {
            store.read(buf).poll(cx)
        } else {
            let mut decrypt_fut = self.decrypt_fut.lock().unwrap();

            let mut fut = match decrypt_fut.take() {
                Some(fut) => fut,
                None => {
                    let core = self.core.clone();
                    let core = Box::pin(core);
                    self.cipher.decrypt(core, buf.len())
                }
            };

            match fut.poll(cx)? {
                Poll::Ready(data) if data.len() <= buf.len() => {
                    unsafe { std::ptr::copy(data.as_ptr(), buf.as_mut_ptr(), data.len()) }

                    Poll::Ready(Ok(data.len()))
                }
                Poll::Ready(data) => {
                    let total = buf.len();
                    match Pin::new(&mut store).poll_write(cx, &data[total..])? {
                        Poll::Pending => Poll::Pending,
                        Poll::Ready(_) => {
                            // let mut cur = Cursor::new(buf);
                            // cur.write_all(&data[..total])?;

                            unsafe {
                                std::ptr::copy(data.as_ptr(), buf.as_mut_ptr(), total);
                            }

                            store.push_back(&data[total..]);

                            Poll::Ready(Ok(total))
                        }
                    }
                }
                Poll::Pending => {
                    *decrypt_fut = Some(fut);
                    Poll::Pending
                }
            }
        }
    }
}

impl<T> AsyncWrite for Crypt<T, Box<DynCipher>>
where
    T: AsyncWrite + Clone + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut encrypt_fut = self.encrypt_fut.lock().unwrap();

        let mut fut = match encrypt_fut.take() {
            Some(fut) => fut,
            None => {
                let core = self.core.clone();
                let core = Box::pin(core);
                self.cipher.encrypt(core, buf)
            }
        };

        match fut.poll(cx)? {
            Poll::Ready(n) => Poll::Ready(Ok(n)),
            Poll::Pending => {
                *encrypt_fut = Some(fut);
                Poll::Pending
            }
        }
    }

    #[inline]
    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.core).poll_flush(cx)
    }

    #[inline]
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.core).poll_close(cx)
    }
}

impl Xor {
    #[inline]
    pub fn new(num: u8) -> Self {
        Self { num }
    }
}

impl Cipher for Xor {
    fn decrypt(
        &self,
        mut io: Pin<Box<dyn AsyncRead + Unpin + Send>>,
        buf_size: usize,
    ) -> Pin<Box<dyn Future<Output = std::io::Result<Vec<u8>>> + Send>> {
        let num = self.num.clone();
        Box::pin(async move {
            let mut buf = Vec::with_capacity(buf_size);

            unsafe {
                buf.set_len(buf_size);
            }

            let n = io.read(&mut buf).await?;

            Ok(buf[..n].iter().map(|n| num ^ n).collect::<Vec<u8>>())
        })
    }

    fn encrypt(
        &self,
        mut io: Pin<Box<dyn AsyncWrite + Unpin + Send>>,
        buf: &[u8],
    ) -> Pin<Box<dyn Future<Output = std::io::Result<usize>> + Send>> {
        let num = self.num.clone();
        let len = buf.len();
        let buf = buf.into_iter().map(|n| num ^ n).collect::<Vec<u8>>();

        Box::pin(async move {
            io.write_all(&buf).await?;
            Ok(len)
        })
    }
}
