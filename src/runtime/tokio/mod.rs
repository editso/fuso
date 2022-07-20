use std::{net::SocketAddr, pin::Pin, sync::Arc, task::Poll};

use async_mutex::Mutex;
use tokio::net::{TcpListener, TcpStream};

use crate::{
    client::{self, Mapper},
    kcp,
    penetrate::SocksClientUdpForward,
    ready, server, Accepter, Addr, ClientFactory, Executor, Factory, FactoryWrapper, FusoStream,
    ServerFactory, Socket, SocketKind, Task, Transfer, UdpSocket,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

#[derive(Clone, Copy)]
pub struct TokioExecutor;
pub struct TokioTcpListener(tokio::net::TcpListener);
pub struct TokioAccepter;
pub struct TokioConnector(Arc<Mutex<Option<kcp::KcpConnector<Arc<tokio::net::UdpSocket>, TokioExecutor>>>>);

pub struct TokioUdpSocket;

pub struct TokioPenetrateConnector;

pub struct TokioUdpServerFactory;
pub struct UdpForwardFactory;
pub struct UdpForwardClientFactory;

impl Executor for TokioExecutor {
    fn spawn<F, O>(&self, fut: F) -> Task<O>
    where
        F: std::future::Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        Task::Tokio(tokio::spawn(fut))
    }
}

impl Factory<Socket> for TokioAccepter {
    type Output = BoxedFuture<TokioTcpListener>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            Ok({
                if socket.is_tcp() {
                    TcpListener::bind(format!("{}", socket.addr()))
                        .await
                        .map_err(|e| {
                            log::warn!("tcp bind failed addr={}, err={}", socket.addr(), e);
                            e
                        })
                        .map(|tcp| TokioTcpListener(tcp))?
                } else {
                    unimplemented!()
                }
            })
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
        let kcp = self.0.clone();

        Box::pin(async move {
            Ok({
                let addr = socket.addr();

                if !socket.is_mixed() {
                    tokio::net::TcpStream::connect(format!("{}", addr))
                        .await?
                        .transfer()
                } else {
                    let mut kcp = kcp.lock().await;

                    if kcp.is_none() {
                        let addr = socket.addr();
                        let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
                        udp.connect(format!("{}", addr)).await?;
                        *kcp = Some(kcp::KcpConnector::new(Arc::new(udp), TokioExecutor));
                    }

                    if kcp.is_some() && socket.is_ufd() {
                        kcp.as_ref().unwrap().connect().await?.transfer()
                    } else {
                        tokio::net::TcpStream::connect(format!("{}", addr))
                            .await
                            .map_err(|e| {
                                log::warn!("connect to {} failed err={}", socket, e);
                                e
                            })?
                            .transfer()
                    }
                }
            })
        })
    }
}

impl ServerFactory<TokioAccepter, TokioConnector> {
    pub fn with_tokio() -> Self {
        ServerFactory {
            accepter_factory: Arc::new(TokioAccepter),
            connector_factory: Arc::new(TokioConnector(Default::default())),
        }
    }
}

impl ClientFactory<TokioConnector> {
    pub fn with_tokio() -> Self {
        ClientFactory {
            server_socket: Default::default(),
            connect_factory: Arc::new(TokioConnector(Default::default())),
        }
    }
}

pub fn builder_server_with_tokio(
) -> server::ServerBuilder<TokioExecutor, TokioAccepter, TokioConnector, FusoStream> {
    server::ServerBuilder {
        is_mixed: false,
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
            let addr = socket.addr();
            match socket.kind() {
                SocketKind::Tcp => Ok(Mapper::Forward(
                    TcpStream::connect(format!("{}", addr))
                        .await
                        .map_err(|e| {
                            log::warn!("failed to connect {}", addr);
                            e
                        })?
                        .transfer(),
                )),
                SocketKind::Ufd => {
                    let factory = FactoryWrapper::wrap(UdpForwardClientFactory);

                    Ok(Mapper::Consume(FactoryWrapper::wrap(
                        SocksClientUdpForward(factory),
                    )))
                }
                _ => unimplemented!(),
            }
        })
    }
}

impl UdpSocket for tokio::net::UdpSocket {
    fn poll_recv_from(
        self: Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> Poll<crate::Result<std::net::SocketAddr>> {
        match ready!(tokio::net::UdpSocket::poll_recv_from(&self, cx, buf)) {
            Err(e) => Poll::Ready(Err(e.into())),
            Ok(addr) => Poll::Ready(Ok(addr)),
        }
    }

    fn poll_recv(
        self: Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> Poll<crate::Result<()>> {
        match ready!(tokio::net::UdpSocket::poll_recv(&self, cx, buf)) {
            Err(e) => Poll::Ready(Err(e.into())),
            Ok(()) => Poll::Ready(Ok(())),
        }
    }

    fn poll_send(
        self: Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        Poll::Ready({
            ready!(tokio::net::UdpSocket::poll_send(&self, cx, buf)).map_err(Into::into)
        })
    }

    fn poll_send_to(
        self: Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        addr: &std::net::SocketAddr,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        Poll::Ready({
            ready!(tokio::net::UdpSocket::poll_send_to(
                &self,
                cx,
                buf,
                addr.clone()
            ))
            .map_err(Into::into)
        })
    }
}

impl Factory<()> for UdpForwardFactory {
    type Output = BoxedFuture<(SocketAddr, tokio::net::UdpSocket)>;

    fn call(&self, _: ()) -> Self::Output {
        Box::pin(async move {
            let udp = tokio::net::UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;
            let addr = udp.local_addr()?;

            log::debug!("udp listening on {}", addr);

            Ok((addr, udp))
        })
    }
}

impl Factory<Addr> for UdpForwardClientFactory {
    type Output = BoxedFuture<(SocketAddr, tokio::net::UdpSocket)>;

    fn call(&self, addr: Addr) -> Self::Output {
        Box::pin(async move {
            let udp = tokio::net::UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).await?;

            udp.connect(format!("{}", addr)).await?;

            let bind_addr = udp.local_addr()?;

            log::debug!("try connect to udp {}", addr);

            Ok((bind_addr, udp))
        })
    }
}

impl Factory<Socket> for TokioUdpServerFactory {
    type Output = BoxedFuture<Arc<tokio::net::UdpSocket>>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            Ok({
                if socket.is_mixed() || socket.is_kcp() || socket.is_udp() {
                    Arc::new({
                        tokio::net::UdpSocket::bind(format!("{}", socket.addr()))
                            .await
                            .map_err(|e| {
                                log::warn!("udp bind failed addr={}, err={}", socket.addr(), e);
                                e
                            })?
                    })
                } else {
                    unimplemented!()
                }
            })
        })
    }
}
