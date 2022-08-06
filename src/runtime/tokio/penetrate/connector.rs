use std::{
    net::{SocketAddr, ToSocketAddrs},
    pin::Pin,
    sync::Arc,
};

use tokio::net::TcpStream;

use crate::{
    client::Route,
    kcp::KcpConnector,
    penetrate::SocksUdpForwardMock,
    udp::{Datagram, VirtualUdpSocket},
    Addr, Address, FusoStream, InnerAddr, InvalidAddr, NetSocket, Provider, Socket, SocketErr,
    SocketKind, ToBoxStream, TokioExecutor, WrappedProvider,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct TokioTcpConnector;

pub struct TokioTcpAndKcpConnector {
    kconnector: Arc<KcpConnector<Arc<tokio::net::UdpSocket>, TokioExecutor>>,
}

pub struct TokioPenetrateConnector {
    udp: Arc<Datagram<Arc<tokio::net::UdpSocket>, TokioExecutor>>,
}

pub struct UdpForwardClientProvider(Arc<Datagram<Arc<tokio::net::UdpSocket>, TokioExecutor>>);

impl TokioPenetrateConnector {
    pub async fn new() -> crate::Result<Self> {
        Ok(Self {
            udp: Arc::new({
                Datagram::new(
                    Arc::new(tokio::net::UdpSocket::bind("0.0.0.0:0").await?),
                    TokioExecutor,
                )?
            }),
        })
    }
}

impl Provider<Socket> for TokioTcpConnector {
    type Output = BoxedFuture<FusoStream>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            Ok({
                tokio::net::TcpStream::connect(socket.as_string())
                    .await?
                    .into_boxed_stream()
            })
        })
    }
}

impl Provider<Socket> for TokioTcpAndKcpConnector {
    type Output = BoxedFuture<FusoStream>;

    fn call(&self, socket: Socket) -> Self::Output {
        let kconnector = self.kconnector.clone();
        Box::pin(async move {
            Ok({
                if socket.is_tcp() {
                    tokio::net::TcpStream::connect(socket.as_string())
                        .await?
                        .into_boxed_stream()
                } else {
                    kconnector.connect().await?.into_boxed_stream()
                }
            })
        })
    }
}

impl Provider<Socket> for TokioPenetrateConnector {
    type Output = BoxedFuture<Route<FusoStream>>;

    fn call(&self, socket: Socket) -> Self::Output {
        let udp = self.udp.clone();
        Box::pin(async move {
            match socket.kind() {
                SocketKind::Tcp => Ok(Route::Forward(
                    TcpStream::connect(socket.as_string())
                        .await?
                        .into_boxed_stream(),
                )),
                SocketKind::Ufd => {
                    let provider = WrappedProvider::wrap(UdpForwardClientProvider(udp));

                    Ok(Route::Provider(WrappedProvider::wrap(
                        SocksUdpForwardMock(provider),
                    )))
                }
                _ => Err(SocketErr::NotSupport(socket).into()),
            }
        })
    }
}

impl Provider<Addr> for UdpForwardClientProvider {
    type Output = BoxedFuture<(SocketAddr, VirtualUdpSocket<Arc<tokio::net::UdpSocket>>)>;

    fn call(&self, addr: Addr) -> Self::Output {
        let udp = self.0.clone();

        Box::pin(async move {
            log::debug!("try connect to udp {}", addr);

            let addr = addr
                .as_string()
                .to_socket_addrs()?
                .next()
                .ok_or(InvalidAddr::Domain(addr.as_string()))?;

            let udp = udp.connect(addr).await?;
            match udp.local_addr()? {
                Address::One(socket) => match socket.into_addr().into_inner() {
                    InnerAddr::Socket(addr) => return Ok((addr, udp)),
                    _ => {}
                },
                _ => {}
            }
            unsafe { std::hint::unreachable_unchecked() }
        })
    }
}
