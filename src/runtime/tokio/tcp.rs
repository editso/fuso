use std::{net::SocketAddr, task::Poll};

use tokio::io::ReadBuf;

use crate::{
    core::{
        accepter::{Accepter, BoxedAccepter},
        io,
        net::{TcpListener, TcpProvider, TcpStream},
        BoxedFuture, BoxedStream, Provider,
    },
    error,
};

impl io::AsyncRead for tokio::net::TcpStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        let mut buf = ReadBuf::new(buf);
        match tokio::io::AsyncRead::poll_read(self, cx, &mut buf)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(()) => Poll::Ready(Ok(buf.filled().len())),
        }
    }
}

impl io::AsyncWrite for tokio::net::TcpStream {
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<()>> {
        tokio::io::AsyncWrite::poll_flush(self, cx).map_err(Into::into)
    }

    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::error::Result<usize>> {
        tokio::io::AsyncWrite::poll_write(self, cx, buf).map_err(Into::into)
    }
}

impl TcpStream {
    pub async fn connect<A>(addr: A) -> error::Result<Self>
    where
        A: tokio::net::ToSocketAddrs,
    {
        let stream = tokio::net::TcpStream::connect(addr).await?;
        Ok(TcpStream {
            stream: BoxedStream::new(stream),
        })
    }
}

#[cfg(feature = "fuso-rt-tokio")]
impl TcpListener {
    pub async fn bind<A>(addr: A) -> error::Result<Self>
    where
        A: tokio::net::ToSocketAddrs,
    {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        Ok(TcpListener {
            accepter: BoxedAccepter::new(listener),
        })
    }
}

pub struct TokioTcpProver;

impl TcpProvider for TokioTcpProver {
    type Listener = Self;

    type Connector = Self;
}

impl Provider<BoxedFuture<'static, error::Result<BoxedStream<'static>>>> for TokioTcpProver {
    type Arg = SocketAddr;

    fn call(addr: Self::Arg) -> BoxedFuture<'static, error::Result<BoxedStream<'static>>> {
        Box::pin(async move {
            Ok(BoxedStream::new(
                tokio::net::TcpStream::connect(addr).await?,
            ))
        })
    }
}

impl
    Provider<
        BoxedFuture<
            'static,
            error::Result<BoxedAccepter<'static, (SocketAddr, BoxedStream<'static>)>>,
        >,
    > for TokioTcpProver
{
    type Arg = SocketAddr;

    fn call(
        addr: Self::Arg,
    ) -> BoxedFuture<
        'static,
        error::Result<BoxedAccepter<'static, (SocketAddr, BoxedStream<'static>)>>,
    > {
        Box::pin(async move {
            Ok(BoxedAccepter::new(
                tokio::net::TcpListener::bind(addr).await?,
            ))
        })
    }
}

impl Accepter for tokio::net::TcpListener {
    type Output = (std::net::SocketAddr, BoxedStream<'static>);
    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<error::Result<Self::Output>> {
        match tokio::net::TcpListener::poll_accept(&*self, ctx)? {
            std::task::Poll::Pending => Poll::Pending,
            std::task::Poll::Ready((stream, addr)) => {
                Poll::Ready(Ok((addr, BoxedStream::new(stream))))
            }
        }
    }
}
