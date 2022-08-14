mod penetrate;
mod udp;

use std::{pin::Pin, sync::Arc, task::Poll};

use futures::Future;
use std::net::SocketAddr;
pub use udp::*;

pub use penetrate::*;

use crate::{
    client, kcp, server, Accepter, Address, ClientProvider, Executor, FusoStream, NetSocket,
    Observer, Provider, Socket, SocketErr, Task, ToBoxStream,
};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

#[derive(Clone)]
pub struct FusoExecutor;
pub struct FusoAccepter;
pub struct FusoConnector(Arc<kcp::KcpConnector<Arc<SmolUdpSocket>, FusoExecutor>>);
pub struct FusoUdpServerProvider;
pub struct FusoUdpForwardProvider;

unsafe impl Sync for SmolUdpSocket {}
unsafe impl Send for SmolUdpSocket {}

pub struct FusoTcpListener {
    smol_tcp: smol::net::TcpListener,
    accept_fut: Option<BoxedFuture<FusoStream>>,
}

unsafe impl Send for FusoTcpListener {}
unsafe impl Sync for FusoTcpListener {}

fn spawn<F, O>(fut: F) -> smol::Task<O>
where
    F: Future<Output = O> + Send + 'static,
    O: Send + 'static,
{
    static GLOBAL: once_cell::sync::Lazy<smol::Executor<'_>> = once_cell::sync::Lazy::new(|| {
        let num_cpus = num_cpus::get().max(2);
        log::info!("use {} threads to process tasks", num_cpus);
        for n in 0..num_cpus {
            drop({
                std::thread::Builder::new()
                    .name(format!("fuso-{}", n))
                    .spawn({
                        log::debug!("task thread {} started", n);
                        move || loop {
                            if let Err(_) = std::panic::catch_unwind(|| {
                                smol::block_on(GLOBAL.run(smol::future::pending::<()>()))
                            }) {
                                log::error!("smol-{}", n);
                            };
                        }
                    })
            });
        }
        smol::Executor::new()
    });

    GLOBAL.spawn(fut)
}

impl Executor for FusoExecutor {
    fn spawn<F, O>(&self, fut: F) -> crate::Task<O>
    where
        F: Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        let task = Arc::new(smol::lock::Mutex::new(Some(spawn(fut))));
        Task {
            abort_fn: Some(Box::new({
                let task = task.clone();
                move || {
                    spawn(async move {
                        if let Some(task) = task.lock().await.take() {
                            task.cancel().await;
                        }
                    })
                    .detach()
                }
            })),
            detach_fn: Some(Box::new(move || {
                spawn(async move {
                    if let Some(task) = task.lock().await.take() {
                        task.detach();
                    }
                })
                .detach()
            })),
            _marked: std::marker::PhantomData,
        }
    }
}

impl Provider<Socket> for FusoAccepter {
    type Output = BoxedFuture<FusoTcpListener>;
    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            Ok(FusoTcpListener {
                smol_tcp: smol::net::TcpListener::bind(socket.as_string()).await?,
                accept_fut: None,
            })
        })
    }
}

impl NetSocket for FusoTcpListener {
    fn peer_addr(&self) -> crate::Result<crate::Address> {
        Ok(Address::One(Socket::tcp(self.smol_tcp.local_addr()?)))
    }

    fn local_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::tcp(self.smol_tcp.local_addr()?)))
    }
}

impl NetSocket for smol::net::TcpStream {
    fn peer_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::tcp((*self).peer_addr()?)))
    }

    fn local_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::tcp((*self).local_addr()?)))
    }
}

impl Accepter for FusoTcpListener {
    type Stream = FusoStream;
    fn poll_accept(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<Self::Stream>> {
        let mut fut = match self.accept_fut.take() {
            Some(fut) => fut,
            None => {
                let smol_tcp = self.smol_tcp.clone();
                Box::pin(async move {
                    let (stream, addr) = smol_tcp.accept().await?;
                    log::debug!("accept connection from {}", addr);
                    Ok(stream.into_boxed_stream())
                })
            }
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(ready) => Poll::Ready(ready),
            Poll::Pending => {
                drop(std::mem::replace(&mut self.accept_fut, Some(fut)));
                Poll::Pending
            }
        }
    }
}

impl Provider<Socket> for FusoConnector {
    type Output = BoxedFuture<FusoStream>;
    fn call(&self, socket: Socket) -> Self::Output {
        let kcp = self.0.clone();
        Box::pin(async move {
            if socket.is_tcp() {
                smol::net::TcpStream::connect(socket.as_string())
                    .await
                    .map(ToBoxStream::into_boxed_stream)
                    .map_err(Into::into)
            } else if socket.is_kcp() {
                kcp.core().connect(socket.as_string()).await?;
                kcp.connect()
                    .await
                    .map(ToBoxStream::into_boxed_stream)
                    .map_err(Into::into)
            } else {
                Err(SocketErr::NotSupport(socket).into())
            }
        })
    }
}

impl Provider<Socket> for FusoUdpServerProvider {
    type Output = BoxedFuture<Arc<SmolUdpSocket>>;
    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move {
            SmolUdpSocket::bind(socket.as_string())
                .await
                .map(|(_, udp_server)| Arc::new(udp_server))
                .map_err(Into::into)
        })
    }
}

impl Provider<()> for FusoUdpForwardProvider {
    type Output = BoxedFuture<(SocketAddr, SmolUdpSocket)>;
    fn call(&self, _: ()) -> Self::Output {
        Box::pin(async move { SmolUdpSocket::bind("0.0.0.0:0").await.map_err(Into::into) })
    }
}

#[inline]
pub fn block_on<F>(fut: F) -> crate::Result<()>
where
    F: Future<Output = crate::Result<()>> + Send,
{
    smol::block_on(fut)
}

pub fn builder_server<O>(
    observer: O,
) -> server::ServerBuilder<FusoExecutor, FusoAccepter, FusoStream, O>
where
    O: Observer + Send + Sync + 'static,
{
    server::ServerBuilder {
        is_mixed: false,
        executor: FusoExecutor,
        handshake: None,
        observer: Some(Arc::new(observer)),
        server_provider: Arc::new(FusoAccepter),
    }
}

pub async fn builder_client(
) -> crate::Result<client::ClientBuilder<FusoExecutor, FusoConnector, FusoStream>> {
    Ok(client::ClientBuilder {
        executor: FusoExecutor,
        handshake: None,
        retry_delay: None,
        maximum_retries: None,
        client_provider: ClientProvider {
            server_address: Default::default(),
            connect_provider: Arc::new({
                let (_, udp_server) = SmolUdpSocket::bind("0.0.0.0:0").await?;
                FusoConnector(Arc::new(kcp::KcpConnector::new(
                    Arc::new(udp_server),
                    FusoExecutor,
                )))
            }),
        },
    })
}
