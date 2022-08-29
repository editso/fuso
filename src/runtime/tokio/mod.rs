mod penetrate;
pub use penetrate::*;

use std::{future::Future, net::SocketAddr, pin::Pin, sync::Arc, task::Poll};

use tokio::net::TcpListener;

use crate::{
    client::{self},
    kcp::{self},
    ready, server, Accepter, Address, ClientProvider, Executor, FusoStream, Kind, NetSocket,
    Webhook, Provider, Socket, SocketErr, Task, ToBoxStream, UdpSocket,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

#[derive(Clone, Copy)]
pub struct FusoExecutor;
pub struct FusoTcpListener(tokio::net::TcpListener);
pub struct FusoAccepter;
pub struct FusoConnector(Arc<kcp::KcpConnector<Arc<tokio::net::UdpSocket>, FusoExecutor>>);

pub struct FusoUdpSocket;

pub struct FusoUdpServerProvider;
pub struct FusoUdpForwardProvider;

impl Executor for FusoExecutor {
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

impl Provider<Socket> for FusoAccepter {
    type Output = BoxedFuture<FusoTcpListener>;

    fn call(&self, socket: Socket) -> Self::Output {
        if socket.is_tcp() || socket.is_mixed() {
            Box::pin(async move {
                Ok({
                    TcpListener::bind(socket.as_string())
                        .await
                        .map(|tcp| FusoTcpListener(tcp))?
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

impl NetSocket for FusoTcpListener {
    fn local_addr(&self) -> crate::Result<crate::Address> {
        Ok(Address::One(Socket::tcp(self.0.local_addr()?)))
    }

    fn peer_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::tcp(self.0.local_addr()?)))
    }
}

impl Accepter for FusoTcpListener {
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

impl Provider<Socket> for FusoConnector {
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
                    kcp.core().connect(socket.as_string()).await?;
                    kcp.connect().await?.into_boxed_stream()
                } else {
                    return Err(SocketErr::NotSupport(socket).into());
                }
            })
        })
    }
}

impl ClientProvider<FusoConnector> {
    pub async fn with_tokio() -> crate::Result<Self> {
        Ok(ClientProvider {
            server_address: Default::default(),
            connect_provider: Arc::new({
                let udp = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
                FusoConnector(Arc::new(kcp::KcpConnector::new(
                    Arc::new(udp),
                    FusoExecutor,
                )))
            }),
        })
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

impl Provider<()> for FusoUdpForwardProvider {
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

impl Provider<Socket> for FusoUdpServerProvider {
    type Output = BoxedFuture<Arc<tokio::net::UdpSocket>>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            Ok(Arc::new({
                tokio::net::UdpSocket::bind(socket.as_string())
                    .await
                    .map_err(|e| {
                        log::warn!("udp bind failed addr={}, err={}", socket.addr(), e);
                        e
                    })?
            }))
        })
    }
}

pub fn block_on<F>(fut: F) -> crate::Result<()>
where
    F: Future<Output = crate::Result<()>> + Send,
{
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(fut)
}

pub fn builder_server<O>(
    webhook: O,
) -> server::ServerBuilder<FusoExecutor, FusoAccepter, FusoStream, O>
where
    O: Webhook + Send + Sync + 'static,
{
    server::ServerBuilder {
        is_mixed: false,
        executor: FusoExecutor,
        handshake: None,
        webhook: Some(Arc::new(webhook)),
        server_provider: Arc::new(FusoAccepter),
    }
}

pub async fn builder_client(
) -> crate::Result<client::ClientBuilder<FusoExecutor, FusoConnector, FusoStream>> {
    Ok(client::ClientBuilder {
        executor: FusoExecutor,
        handshake: None,
        client_provider: ClientProvider::with_tokio().await?,
        retry_delay: None,
        maximum_retries: None,
    })
}
