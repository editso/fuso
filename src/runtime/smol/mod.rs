use std::{net::SocketAddr, pin::Pin, sync::Arc, task::Poll};

use futures::Future;
use smol::net::{AsyncToSocketAddrs, TcpStream};

use crate::{
    client::{ClientBuilder, Mapper},
    server::ServerBuilder,
    Accepter, ClientFactory, Executor, Factory, FusoStream, ServerFactory, Socket, Transfer,
};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

#[derive(Default, Clone)]
pub struct SmolExecutor;
pub struct SmolListener;
pub struct SmolAccepter;

pub struct SmolPenetrateConnector;

pub struct SmolTcpListener {
    tcp: smol::net::TcpListener,
    accept_fut: Option<BoxedFuture<(TcpStream, SocketAddr)>>,
}

pub struct SmolConnector;

impl SmolTcpListener {
    pub async fn bind<A>(addr: A) -> std::io::Result<Self>
    where
        A: AsyncToSocketAddrs,
    {
        let tcp = smol::net::TcpListener::bind(addr).await?;

        Ok(Self {
            tcp,
            accept_fut: None,
        })
    }
}

impl Executor for SmolExecutor {
    fn spawn<F, O>(&self, fut: F)
    where
        F: futures::Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        smol::spawn(fut).detach();
    }
}

impl Factory<Socket> for SmolAccepter {
    type Output = BoxedFuture<SmolTcpListener>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            match socket {
                Socket::Tcp(addr) => SmolTcpListener::bind(format!("{}", addr))
                    .await
                    .map_err(Into::into),
                _ => todo!(),
            }
        })
    }
}

impl Accepter for SmolTcpListener {
    type Stream = FusoStream;

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        let mut fut = match self.accept_fut.take() {
            Some(fut) => fut,
            None => {
                let tcp = self.tcp.clone();
                Box::pin(async move { tcp.accept().await.map_err(Into::into) })
            }
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(stream) => Poll::Ready(stream.map(|(stream, addr)| {
                log::debug!("accept connection from {}", addr);
                stream.transfer()
            })),
            Poll::Pending => {
                drop(std::mem::replace(&mut self.accept_fut, Some(fut)));
                Poll::Pending
            }
        }
    }
}

impl Factory<Socket> for SmolConnector {
    type Output = BoxedFuture<FusoStream>;
    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            match socket {
                Socket::Udp(_) => todo!(),
                Socket::Tcp(addr) => Ok(TcpStream::connect(format!("{}", addr)).await?.transfer()),
                Socket::Kcp(_) => todo!(),
                Socket::Quic(_) => todo!(),
            }
        })
    }
}

impl Factory<Socket> for SmolPenetrateConnector {
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

impl ServerFactory<SmolAccepter, SmolConnector> {
    pub fn with_smol() -> Self {
        Self {
            accepter_factory: Arc::new(SmolAccepter),
            connector_factory: Arc::new(SmolConnector),
        }
    }
}

impl ClientFactory<SmolConnector> {
    pub fn with_smol() -> Self {
        Self {
            connect_factory: Arc::new(SmolConnector),
        }
    }
}

pub fn builder_server_with_smol(
) -> ServerBuilder<SmolExecutor, SmolAccepter, SmolConnector, FusoStream> {
    ServerBuilder {
        executor: SmolExecutor,
        handshake: None,
        server_factory: ServerFactory::with_smol(),
    }
}

pub fn builder_client_with_smol() -> ClientBuilder<SmolExecutor, SmolConnector, FusoStream> {
    ClientBuilder {
        executor: SmolExecutor,
        handshake: None,
        client_factory: ClientFactory::with_smol(),
    }
}
