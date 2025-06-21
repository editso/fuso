use std::{
    future::Future,
    io::{Cursor, Read, Write},
    net::SocketAddr,
    pin::Pin,
    task::Poll,
};

use self::io::{AsyncRead, AsyncWrite};

pub mod accepter;
pub mod channel;
pub mod connection;
pub mod connector;
pub mod future;
pub mod handshake;
pub mod io;
pub mod net;
pub mod processor;
pub mod protocol;
pub mod rpc;
pub mod split;
pub mod stream;
pub mod task;
pub mod token;
pub mod transfer;

pub type BoxedFuture<'a, O> = Pin<Box<dyn Future<Output = O> + Send + 'a>>;

pub trait Provider<R> {
    type Arg;

    fn call(arg: Self::Arg) -> R;
}

pub trait Stream: io::AsyncRead + io::AsyncWrite {}

pub struct Connection<'a> {
    addr: SocketAddr,
    stream: AbstractStream<'a>,
    cursor: Option<Cursor<Vec<u8>>>,
    marked: Option<Cursor<Vec<u8>>>,
}

pub struct AbstractStream<'a>(Box<dyn Stream + Unpin + Send + 'a>);

impl<'a> AbstractStream<'a> {
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream + Unpin + Send + 'a,
    {
        Self(Box::new(stream))
    }
}

impl<T> Stream for T where T: io::AsyncRead + io::AsyncWrite + Unpin {}

impl<'a> io::AsyncRead for AbstractStream<'a> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        Pin::new(&mut *self.0).poll_read(cx, buf)
    }
}

impl<'a> io::AsyncWrite for AbstractStream<'a> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        Pin::new(&mut *self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<()>> {
        Pin::new(&mut *self.0).poll_flush(cx)
    }
}

impl<'a> From<(SocketAddr, AbstractStream<'a>)> for Connection<'a> {
    fn from(value: (SocketAddr, AbstractStream<'a>)) -> Self {
        Self {
            addr: value.0,
            stream: value.1,
            cursor: Default::default(),
            marked: Default::default(),
        }
    }
}

impl<'a> Connection<'a> {
    pub fn new<S>(addr: SocketAddr, stream: S) -> Self
    where
        S: Into<AbstractStream<'a>>,
    {
        Self {
            addr,
            stream: stream.into(),
            cursor: None,
            marked: None,
        }
    }

    pub fn addr(&self) -> &SocketAddr {
        &self.addr
    }

    pub fn mark(&mut self) {
        match self.marked.take() {
            None => drop(self.marked.replace(Default::default())),
            Some(marked) => match self.cursor.as_mut() {
                None => drop(self.cursor.replace(marked)),
                Some(cursor) => {
                    let _ = cursor.write_all(&marked.into_inner());
                }
            },
        }
    }

    pub fn reset(&mut self) {
        if let Some(mut marked) = self.marked.take() {
            match self.cursor.take() {
                None => {
                    marked.set_position(0);
                    drop(self.cursor.replace(marked))
                }
                Some(cursor) => {
                    let pos = cursor.position();
                    let buf = &cursor.into_inner()[pos as usize..];
                    let mut cursor = Cursor::new(buf.to_vec());
                    let _ = cursor.write_all(&marked.into_inner());
                    self.cursor.replace(cursor);
                }
            }
        }
    }

    pub fn discard(&mut self) {
        drop(self.cursor.take());
        drop(self.marked.take());
    }
}

impl<'a> AsyncRead for Connection<'a> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        let n = match self.cursor.as_mut() {
            None => 0,
            Some(cursor) => cursor.read(buf)?,
        };

        let n = if n <= 0 {
            match Pin::new(&mut self.stream).poll_read(cx, buf)? {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(n) => n,
            }
        } else {
            n
        };

        if let Some(marked) = self.marked.as_mut() {
            marked.write_all(&buf[..n])?;
        }

        Poll::Ready(Ok(n))
    }
}

impl<'a> AsyncWrite for Connection<'a> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        self.discard();
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
}
