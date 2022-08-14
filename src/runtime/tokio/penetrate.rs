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
    Addr, FusoExecutor, FusoStream, InvalidAddr, Provider, Socket, SocketErr, SocketKind,
    ToBoxStream, WrappedProvider,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct TokioTcpConnector;

pub struct FusoTcpAndKcpConnector {
    kconnector: Arc<KcpConnector<Arc<tokio::net::UdpSocket>, FusoExecutor>>,
}

pub struct FusoPenetrateConnector {
    udp: Arc<Datagram<Arc<tokio::net::UdpSocket>, FusoExecutor>>,
}

pub struct UdpForwardClientProvider(Arc<Datagram<Arc<tokio::net::UdpSocket>, FusoExecutor>>);

impl FusoPenetrateConnector {
    pub async fn new() -> crate::Result<Self> {
        let udp_server = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
        let listen_addr = udp_server.local_addr()?;
        Ok(Self {
            udp: Arc::new(Datagram::new(
                Arc::new(udp_server),
                listen_addr,
                FusoExecutor,
            )?),
        })
    }
}

impl Provider<Socket> for TokioTcpConnector {
    type Output = BoxedFuture<FusoStream>;

    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            tokio::net::TcpStream::connect(socket.as_string())
                .await
                .map(ToBoxStream::into_boxed_stream)
                .map_err(Into::into)
        })
    }
}

impl Provider<Socket> for FusoTcpAndKcpConnector {
    type Output = BoxedFuture<FusoStream>;

    fn call(&self, socket: Socket) -> Self::Output {
        let kconnector = self.kconnector.clone();
        Box::pin(async move {
            if socket.is_tcp() {
                tokio::net::TcpStream::connect(socket.as_string())
                    .await
                    .map(ToBoxStream::into_boxed_stream)
                    .map_err(Into::into)
            } else {
                kconnector
                    .connect()
                    .await
                    .map(ToBoxStream::into_boxed_stream)
                    .map_err(Into::into)
            }
        })
    }
}

impl Provider<Socket> for FusoPenetrateConnector {
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

                    Ok(Route::Provider(WrappedProvider::wrap(SocksUdpForwardMock(
                        provider,
                    ))))
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

            udp.connect(addr).await
        })
    }
}
