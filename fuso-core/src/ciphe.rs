use std::{
    io::{Cursor, Write},
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::Poll,
};

use async_trait::async_trait;
use futures::{AsyncRead, AsyncWrite};

use fuso_api::{Buffer, SafeStream};
use smol::net::TcpStream;

#[async_trait]
pub trait Security<T, O> {
    async fn ciphe(self, t: O) -> Crypt<T, O>;
}

pub trait Cipher {
    fn poll_decode(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<Vec<u8>>>;

    fn poll_encode(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<Vec<u8>>>;
}

#[derive(Clone)]
pub struct Crypt<T, C> {
    buf: Arc<Mutex<Buffer<u8>>>,
    target: T,
    cipher: C,
}

#[derive(Clone)]
pub struct Xor {
    num: u8,
}

impl<C> Crypt<TcpStream, C> {
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.local_addr()
    }

    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.peer_addr()
    }
}

impl<C> Crypt<SafeStream<TcpStream>, C> {
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.local_addr()
    }

    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.target.peer_addr()
    }
}

#[async_trait]
impl<T, C> Security<T, C> for T
where
    T: AsyncWrite + AsyncRead + Send + Sync + 'static,
    C: Cipher + Send + Sync + 'static,
{
    #[inline]
    async fn ciphe(self, c: C) -> Crypt<T, C> {
        Crypt {
            target: self,
            buf: Arc::new(Mutex::new(Buffer::new())),
            cipher: c,
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
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let io_buf = self.buf.clone();

        let mut io_buf = io_buf.lock().unwrap();

        if !io_buf.is_empty() {
            Pin::new(&mut *io_buf).poll_read(cx, buf)
        } else {
            match Pin::new(&mut self.target).poll_read(cx, buf) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Ready(Ok(0)) => Poll::Ready(Ok(0)),
                Poll::Ready(Ok(n)) => match Pin::new(&mut self.cipher).poll_decode(cx, &buf[..n]) {
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
                },
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
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        match Pin::new(&mut self.cipher).poll_encode(cx, buf) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Ready(Ok(data)) => Pin::new(&mut self.target).poll_write(cx, &data),
        }
    }

    #[inline]
    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.target).poll_flush(cx)
    }

    #[inline]
    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.target).poll_close(cx)
    }
}

impl Xor {
    #[inline]
    pub fn new(num: u8) -> Self {
        Self { num }
    }
}

impl Cipher for Xor {
    #[inline]
    fn poll_decode(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<Vec<u8>>> {
        log::debug!("[cipher] decrypt {}", data.len());

        Poll::Ready(Ok(data.iter().map(|e| *e ^ self.num).collect()))
    }

    #[inline]
    fn poll_encode(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<Vec<u8>>> {
        log::debug!("[cipher] encrypt {}", data.len());

        Poll::Ready(Ok(data.iter().map(|e| *e ^ self.num).collect()))
    }
}
