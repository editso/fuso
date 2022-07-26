mod penetrate;
pub use penetrate::connector::*;

use std::{net::SocketAddr, pin::Pin, sync::Arc, task::Poll};

use async_mutex::Mutex;
use tokio::net::TcpListener;

use crate::{
    client::{self},
    kcp, ready, server, Accepter, Address, ClientProvider, Executor, FusoStream, NetSocket,
    Provider, ServerProvider, Socket, SocketErr, Task, ToBoxStream, UdpSocket,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

#[derive(Clone, Copy)]
pub struct TokioExecutor;
pub struct TokioTcpListener(tokio::net::TcpListener);
pub struct TokioAccepter;
pub struct TokioConnector(
    Arc<Mutex<Option<kcp::KcpConnector<Arc<tokio::net::UdpSocket>, TokioExecutor>>>>,
);

pub struct TokioUdpSocket;

pub struct TokioUdpServerProvider;
pub struct UdpForwardProvider;

impl Executor for TokioExecutor {
    fn spawn<F, O>(&self, fut: F) -> Task<O>
    where
        F: std::future::Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        let task = tokio::spawn(fut);

        Task {
            detach_task_fn: None,
            abort_task_fn: Some(Box::new(move || {
                task.abort();
                log::debug!("abort task");
            })),
            _marked: std::marker::PhantomData,
        }
    }
}

impl Provider<Socket> for TokioAccepter {
    type Output = BoxedFuture<TokioTcpListener>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            Ok({
                if socket.is_tcp() {
                    TcpListener::bind(socket.as_string())
                        .await
                        .map(|tcp| TokioTcpListener(tcp))?
                } else {
                    return Err(SocketErr::NotSupport(socket).into());
                }
            })
        })
    }
}

impl NetSocket for tokio::net::TcpStream {
    fn peer_addr(&self) -> crate::Result<Address> {
        Ok(Address::Single(Socket::tcp(self.peer_addr()?)))
    }

    fn local_addr(&self) -> crate::Result<Address> {
        Ok(Address::Single(Socket::tcp(self.local_addr()?)))
    }
}

impl NetSocket for TokioTcpListener {
    fn local_addr(&self) -> crate::Result<crate::Address> {
        Ok(Address::Single(Socket::tcp(self.0.local_addr()?)))
    }

    fn peer_addr(&self) -> crate::Result<Address> {
        Ok(Address::Single(Socket::tcp(self.0.local_addr()?)))
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

                Poll::Ready(Ok(tcp.into_boxed_stream()))
            }
        }
    }
}

impl Provider<Socket> for TokioConnector {
    type Output = BoxedFuture<FusoStream>;

    fn call(&self, socket: Socket) -> Self::Output {
        let kcp = self.0.clone();

        Box::pin(async move {
            Ok({
                if !socket.is_mixed() {
                    tokio::net::TcpStream::connect(socket.as_string())
                        .await?
                        .into_boxed_stream()
                } else {
                    let mut kcp = kcp.lock().await;

                    if kcp.is_none() {
                        let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
                        udp.connect(socket.as_string()).await?;
                        *kcp = Some(kcp::KcpConnector::new(Arc::new(udp), TokioExecutor));
                    }

                    if kcp.is_some() && socket.is_ufd() {
                        kcp.as_ref().unwrap().connect().await?.into_boxed_stream()
                    } else {
                        tokio::net::TcpStream::connect(socket.as_string())
                            .await
                            .map_err(|e| {
                                log::warn!("connect to {} failed err={}", socket, e);
                                e
                            })?
                            .into_boxed_stream()
                    }
                }
            })
        })
    }
}

impl ServerProvider<TokioAccepter, TokioConnector> {
    pub fn with_tokio() -> Self {
        ServerProvider {
            accepter_provider: Arc::new(TokioAccepter),
            connector_provider: Arc::new(TokioConnector(Default::default())),
        }
    }
}

impl ClientProvider<TokioConnector> {
    pub fn with_tokio() -> Self {
        ClientProvider {
            server_socket: Default::default(),
            connect_provider: Arc::new(TokioConnector(Default::default())),
        }
    }
}

pub fn builder_server_with_tokio(
) -> server::ServerBuilder<TokioExecutor, TokioAccepter, TokioConnector, FusoStream> {
    server::ServerBuilder {
        is_mixed: false,
        executor: TokioExecutor,
        handshake: None,
        server_provider: ServerProvider::with_tokio(),
    }
}

pub fn builder_client_with_tokio(
) -> client::ClientBuilder<TokioExecutor, TokioConnector, FusoStream> {
    client::ClientBuilder {
        executor: TokioExecutor,
        handshake: None,
        client_provider: ClientProvider::with_tokio(),
    }
}

impl NetSocket for tokio::net::UdpSocket {
    fn peer_addr(&self) -> crate::Result<Address> {
        Ok(Address::Single(Socket::udp((*self).peer_addr()?)))
    }

    fn local_addr(&self) -> crate::Result<Address> {
        Ok(Address::Single(Socket::udp((*self).local_addr()?)))
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

impl Provider<()> for UdpForwardProvider {
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

impl Provider<Socket> for TokioUdpServerProvider {
    type Output = BoxedFuture<Arc<tokio::net::UdpSocket>>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            Ok({
                if socket.is_mixed() || socket.is_kcp() || socket.is_udp() {
                    Arc::new({
                        tokio::net::UdpSocket::bind(socket.as_string())
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
