use std::{net::SocketAddr, pin::Pin, task::Poll};

use crate::error;

use crate::core::{
    accepter::{Accepter, BoxedAccepter},
    io::{AsyncRead, AsyncWrite},
    BoxedFuture, BoxedStream, Provider,
};

pub trait TcpProvider {
    type Connector: Provider<
        BoxedFuture<'static, error::Result<BoxedStream<'static>>>,
        Arg = SocketAddr,
    >;

    type Listener: Provider<
        BoxedFuture<
            'static,
            error::Result<BoxedAccepter<'static, (SocketAddr, BoxedStream<'static>)>>,
        >,
        Arg = SocketAddr,
    >;
}

pub struct TcpListener {
    pub(crate) accepter: BoxedAccepter<'static, (SocketAddr, BoxedStream<'static>)>,
}

pub struct TcpStream {
    pub(crate) stream: BoxedStream<'static>,
}

impl TcpStream {
    pub async fn connect<P, A>(addr: A) -> error::Result<Self>
    where
        P: TcpProvider,
        A: Into<SocketAddr>,
    {
        Ok(TcpStream {
            stream: P::Connector::call(addr.into()).await?,
        })
    }
}


#[cfg(feature = "fuso-runtime")]
impl TcpListener {
    pub async fn bind_with_provider<P, A>(addr: A) -> error::Result<TcpListener>
    where
        P: TcpProvider,
        A: Into<SocketAddr>,
    {
        Ok(TcpListener {
            accepter: P::Listener::call(addr.into()).await?,
        })
    }
}


impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<error::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<error::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<error::Result<usize>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl Accepter for TcpListener {
    type Output = (SocketAddr, TcpStream);
    fn poll_accept(
        mut self: Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<error::Result<Self::Output>> {
        match Pin::new(&mut self.accepter).poll_accept(ctx)? {
            std::task::Poll::Pending => Poll::Pending,
            std::task::Poll::Ready((addr, stream)) => Poll::Ready(Ok((addr, TcpStream { stream }))),
        }
    }
}
