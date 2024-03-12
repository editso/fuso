use std::{future::Future, net::SocketAddr, pin::Pin};

pub mod accepter;
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

pub type BoxedFuture<'a, O> = Pin<Box<dyn Future<Output = O> + Send + 'a>>;

pub trait Provider<R> {
    type Arg;

    fn call(arg: Self::Arg) -> R;
}

pub trait Stream: io::AsyncRead + io::AsyncWrite {}

pub struct Connection<'a> {
    addr: SocketAddr,
    stream: BoxedStream<'a>,
}

pub struct BoxedStream<'a>(Box<dyn Stream + Unpin + Send + 'a>);

impl<'a> BoxedStream<'a> {
    pub fn new<S>(stream: S) -> Self
    where
        S: Stream + Unpin + Send + 'a,
    {
        Self(Box::new(stream))
    }
}

impl<T> Stream for T where T: io::AsyncRead + io::AsyncWrite + Unpin {}

impl<'a> io::AsyncRead for BoxedStream<'a> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        Pin::new(&mut *self.0).poll_read(cx, buf)
    }
}

impl<'a> io::AsyncWrite for BoxedStream<'a> {
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

impl<'a> From<(SocketAddr, BoxedStream<'a>)> for Connection<'a> {
    fn from(value: (SocketAddr, BoxedStream<'a>)) -> Self {
        Self {
            addr: value.0,
            stream: value.1,
        }
    }
}
