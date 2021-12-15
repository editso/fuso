use std::{
    io::{Cursor, Write},
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    task::Poll,
};

use futures::{AsyncRead, AsyncWrite};
use smol::net::TcpStream;

use crate::{Buffer, DynCipher, Rollback, RollbackEx, UdpStream};

#[derive(Clone)]
pub struct SafeStream<Inner> {
    core: Rollback<Inner, Buffer<u8>>,
    store: Buffer<u8>,
    cipher: Arc<Mutex<Option<Box<DynCipher>>>>,
}

pub trait SafeStreamEx<T> {
    fn as_safe_stream(self) -> SafeStream<T>;
}

impl SafeStreamEx<Self> for TcpStream {
    #[inline]
    fn as_safe_stream(self) -> SafeStream<Self> {
        SafeStream {
            core: self.roll(),
            store: Buffer::new(),
            cipher: Arc::new(Mutex::new(None)),
        }
    }
}

impl SafeStreamEx<Self> for UdpStream {
    #[inline]
    fn as_safe_stream(self) -> SafeStream<Self> {
        SafeStream {
            core: self.roll(),
            store: Buffer::new(),
            cipher: Arc::new(Mutex::new(None)),
        }
    }
}

impl SafeStream<TcpStream> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.core.local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.core.peer_addr()
    }
}

impl SafeStream<UdpStream> {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.core.local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        self.core.peer_addr()
    }
}

impl<Inner> SafeStream<Inner>
where
    Inner: Send + Sync + 'static,
{
    #[inline]
    pub async fn begin(&mut self) -> crate::Result<()> {
        self.core.begin().await
    }

    #[inline]
    pub async fn back(&mut self) -> crate::Result<()> {
        self.core.back().await
    }

    #[inline]
    pub async fn release(&mut self) -> crate::Result<()> {
        self.core.release().await
    }

    #[inline]
    pub fn set_cipher(&mut self, cipher: Box<DynCipher>) -> crate::Result<()> {
        *self.cipher.lock().unwrap() = Some(cipher);
        Ok(())
    }
}

impl<Inner> AsyncRead for SafeStream<Inner>
where
    Inner: Clone + AsyncRead + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut store = self.store.clone();

        if !store.is_empty() {
            Pin::new(&mut store).poll_read(cx, buf)
        } else {
            let cipher = self.cipher.clone();
            let mut cipher = cipher.lock().unwrap();

            if let Some(cipher) = cipher.as_mut() {
                let io = Box::new(&mut self.core as _);
                match cipher.poll_decrypt(io, cx, buf)? {
                    Poll::Ready(packet) if packet.len() <= buf.len() => {
                        let n = packet.len();
                        let mut cur = Cursor::new(buf);
                        let _ = cur.write_all(&packet)?;
                        Poll::Ready(Ok(n))
                    }
                    Poll::Ready(packet) => {
                        let total = buf.len();
                        match Pin::new(&mut store).poll_write(cx, &packet[total..])? {
                            Poll::Pending => Poll::Pending,
                            Poll::Ready(_) => {
                                let mut cur = Cursor::new(buf);
                                cur.write_all(&packet[..total])?;
                                Poll::Ready(Ok(total))
                            }
                        }
                    }
                    Poll::Pending =>Poll::Pending,
                }
            } else {
                Pin::new(&mut self.core).poll_read(cx, buf)
            }
        }
    }
}

impl<Inner> AsyncWrite for SafeStream<Inner>
where
    Inner: Clone + AsyncWrite + Unpin + Send + Sync + 'static,
{
    #[inline]
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let cipher = self.cipher.clone();
        let mut cipher = cipher.lock().unwrap();
        if let Some(cipher) = cipher.as_mut() {
            let io = Box::new(&mut self.core as _);
            cipher.poll_encrypt(io, cx, buf)
        } else {
            Pin::new(&mut self.core).poll_write(cx, buf)
        }
    }

    #[inline]
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.core).poll_flush(cx)
    }

    #[inline]
    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.core).poll_close(cx)
    }
}
