use std::{pin::Pin, sync::Arc, task::Poll};

use tokio::net::{TcpListener, TcpStream};

use crate::{
    client::{self, Outcome},
    listener::Accepter,
    server,
    service::{self, Factory, ServerFactory, Transfer},
    Addr, Executor, FusoStream, Socket,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

#[derive(Clone, Copy)]
pub struct TokioExecutor;

pub struct TokioTcpListener(tokio::net::TcpListener);
pub struct TokioAccepter;
pub struct TokioConnector;
pub struct TokioClientConnector;
pub struct TokioClientForwardConnector;

impl Executor for TokioExecutor {
    fn spawn<F, O>(&self, fut: F)
    where
        F: std::future::Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        tokio::spawn(fut);
    }
}

impl Factory<Socket> for TokioAccepter {
    type Output = BoxedFuture<TokioTcpListener>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            match socket {
                Socket::Tcp(addr) => match addr.inner() {
                    crate::InnerAddr::Socket(addr) => TcpListener::bind(addr)
                        .await
                        .map_err(Into::into)
                        .map(|tcp| TokioTcpListener(tcp)),
                    crate::InnerAddr::Domain(_, _) => unimplemented!(),
                },
                _ => unimplemented!(),
            }
        })
    }
}

impl Accepter for TokioTcpListener {
    type Stream = FusoStream;
    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<Self::Stream>> {
        match self.0.poll_accept(cx) {
            Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok((tcp, addr))) => {
                log::debug!("[tokio::tcp] accept connection from {}", addr);
                Poll::Ready(Ok(tcp.transfer()))
            }
        }
    }
}

impl Factory<Socket> for TokioConnector {
    type Output = BoxedFuture<FusoStream>;

    fn call(&self, cfg: Socket) -> Self::Output {
        Box::pin(async move {
            tokio::net::TcpStream::connect(format!("{}", cfg))
                .await
                .map(Transfer::transfer)
                .map_err(Into::into)
        })
    }
}

impl ServerFactory<TokioAccepter, TokioConnector> {
    pub fn with_tokio() -> Self {
        ServerFactory {
            accepter_factory: Arc::new(TokioAccepter),
            connector_factory: Arc::new(TokioConnector),
        }
    }
}

impl Factory<Socket> for TokioClientConnector {
    type Output = BoxedFuture<FusoStream>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            match socket {
                Socket::Udp(_) => todo!(),
                Socket::Kcp(_) => todo!(),
                Socket::Quic(_) => todo!(),
                Socket::Tcp(addr) => Ok({
                    tokio::net::TcpStream::connect(format!("{}", addr))
                        .await?
                        .transfer()
                }),
            }
        })
    }
}

impl From<tokio::net::TcpStream> for FusoStream {
    fn from(t: tokio::net::TcpStream) -> Self {
        Self::new(t)
    }
}

pub fn builder_server_with_tokio(
) -> server::ServerBuilder<TokioExecutor, TokioAccepter, TokioConnector, FusoStream> {
    server::ServerBuilder {
        executor: TokioExecutor,
        handshake: None,
        server_factory: ServerFactory::with_tokio(),
    }
}

pub fn builder_client_with_tokio(
) -> client::ClientBuilder<TokioExecutor, TokioClientConnector, FusoStream> {
    client::ClientBuilder {
        executor: TokioExecutor,
        handshake: None,
        client_factory: service::ClientFactory {
            connect_factory: Arc::new(TokioClientConnector),
        },
    }
}

impl Factory<Socket> for TokioClientForwardConnector {
    type Output = BoxedFuture<Outcome<FusoStream>>;

    fn call(&self, arg: Socket) -> Self::Output {
        Box::pin(async move {
            Ok({ Outcome::Stream(TcpStream::connect("127.0.0.1:8080").await?.transfer()) })
        })
    }
}
