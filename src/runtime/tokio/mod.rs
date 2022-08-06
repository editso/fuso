mod penetrate;
pub use penetrate::connector::*;

use std::{net::SocketAddr, pin::Pin, sync::Arc, task::Poll};

use async_mutex::Mutex;
use tokio::net::TcpListener;

use crate::{
    client::{self},
    kcp::{self},
    ready, server, Accepter, Address, ClientProvider, Executor, FusoStream, Kind, NetSocket,
    Observer, Provider, Socket, SocketErr, Task, ToBoxStream, UdpSocket,
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
            detach_fn: None,
            abort_fn: Some(Box::new(move || {
                task.abort();
            })),
            _marked: std::marker::PhantomData,
        }
    }
}

impl Provider<Socket> for TokioAccepter {
    type Output = BoxedFuture<TokioTcpListener>;

    fn call(&self, socket: Socket) -> Self::Output {
        if socket.is_tcp() || socket.is_mixed() {
            Box::pin(async move {
                Ok({
                    TcpListener::bind(socket.as_string())
                        .await
                        .map(|tcp| TokioTcpListener(tcp))?
                })
            })
        } else {
            Box::pin(async move { Err(Kind::Unsupported(socket).into()) })
        }
    }
}

impl NetSocket for tokio::net::TcpStream {
    fn peer_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::tcp(self.peer_addr()?)))
    }

    fn local_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::tcp(self.local_addr()?)))
    }
}

impl NetSocket for TokioTcpListener {
    fn local_addr(&self) -> crate::Result<crate::Address> {
        Ok(Address::One(Socket::tcp(self.0.local_addr()?)))
    }

    fn peer_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::tcp(self.0.local_addr()?)))
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
                if socket.is_tcp() {
                    tokio::net::TcpStream::connect(socket.as_string())
                        .await?
                        .into_boxed_stream()
                } else if socket.is_kcp() {
                    let mut kcp = kcp.lock().await;

                    if kcp.is_none() {
                        let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
                        udp.connect(socket.as_string()).await?;
                        *kcp = Some(kcp::KcpConnector::new(Arc::new(udp), TokioExecutor));
                    }

                    kcp.as_ref().unwrap().connect().await?.into_boxed_stream()
                } else {
                    return Err(SocketErr::NotSupport(socket).into());
                }
            })
        })
    }
}

impl ClientProvider<TokioConnector> {
    pub fn with_tokio() -> Self {
        ClientProvider {
            server_address: Default::default(),
            connect_provider: Arc::new(TokioConnector(Default::default())),
        }
    }
}

pub fn builder_server_with_tokio<O>(
    observer: O,
) -> server::ServerBuilder<TokioExecutor, TokioAccepter, FusoStream, O>
where
    O: Observer + Send + Sync + 'static,
{
    server::ServerBuilder {
        is_mixed: false,
        executor: TokioExecutor,
        handshake: None,
        observer: Some(Arc::new(observer)),
        server_provider: Arc::new(TokioAccepter),
    }
}

pub fn builder_client_with_tokio(
) -> client::ClientBuilder<TokioExecutor, TokioConnector, FusoStream> {
    client::ClientBuilder {
        executor: TokioExecutor,
        handshake: None,
        client_provider: ClientProvider::with_tokio(),
        retry_delay: None,
        maximum_retries: None,
    }
}

impl NetSocket for tokio::net::UdpSocket {
    fn peer_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::udp((*self).peer_addr()?)))
    }

    fn local_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::udp((*self).local_addr()?)))
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
