use std::{pin::Pin, sync::Arc, task::Poll};

use tokio::net::{TcpListener, TcpStream};

use crate::{
    client::{self, Mapper},
    server, Accepter, ClientFactory, Executor, Factory, FusoStream, ServerFactory, Socket,
    Transfer,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

#[derive(Clone, Copy)]
pub struct TokioExecutor;
pub struct TokioTcpListener(tokio::net::TcpListener);
pub struct TokioAccepter;
pub struct TokioConnector;
pub struct TokioClientConnector;
pub struct TokioPenetrateConnector;

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
                log::debug!("accept connection from {}", addr);
                Poll::Ready(Ok(tcp.transfer()))
            }
        }
    }
}

impl Factory<Socket> for TokioConnector {
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

impl ServerFactory<TokioAccepter, TokioConnector> {
    pub fn with_tokio() -> Self {
        ServerFactory {
            accepter_factory: Arc::new(TokioAccepter),
            connector_factory: Arc::new(TokioConnector),
        }
    }
}

impl ClientFactory<TokioConnector> {
    pub fn with_tokio() -> Self {
        ClientFactory {
            connect_factory: Arc::new(TokioConnector),
        }
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
) -> client::ClientBuilder<TokioExecutor, TokioConnector, FusoStream> {
    client::ClientBuilder {
        executor: TokioExecutor,
        handshake: None,
        client_factory: ClientFactory::with_tokio(),
    }
}

impl Factory<Socket> for TokioPenetrateConnector {
    type Output = BoxedFuture<Mapper<FusoStream>>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            match socket {
                Socket::Tcp(addr) => Ok(Mapper::Forward(
                    TcpStream::connect(format!("{}", addr)).await?.transfer(),
                )),
                _ => unimplemented!(),
            }
        })
    }
}
