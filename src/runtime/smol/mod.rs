mod penetrate;

use std::{pin::Pin, sync::Arc, task::Poll};

use futures::Future;
use smol::{lock::Mutex, net::AsyncToSocketAddrs};
use std::net::SocketAddr;

pub use penetrate::*;

use crate::{
    client, kcp, server, Accepter, Address, ClientProvider, Executor, FusoStream, NetSocket,
    Observer, Provider, Socket, SocketErr, Task, ToBoxStream, UdpSocket,
};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;
type UFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + 'static>>;

#[derive(Clone)]
pub struct FusoExecutor;
pub struct FusoAccepter;
pub struct FusoConnector(Arc<Mutex<Option<kcp::KcpConnector<Arc<SmolUdpSocket>, FusoExecutor>>>>);
pub struct FusoUdpServerProvider;
pub struct FusoUdpForwardProvider;

pub struct SmolUdpSocket {
    smol_udp: smol::net::UdpSocket,
    recv_fut: std::sync::Mutex<Option<UFuture<usize>>>,
    recv_from_fut: std::sync::Mutex<Option<UFuture<(usize, std::net::SocketAddr)>>>,
    send_fut: std::sync::Mutex<Option<UFuture<usize>>>,
    send_to_fut: std::sync::Mutex<Option<UFuture<usize>>>,
}

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
            Ok({
                if socket.is_tcp() {
                    smol::net::TcpStream::connect(socket.as_string())
                        .await?
                        .into_boxed_stream()
                } else if socket.is_kcp() {
                    let mut kcp = kcp.lock().await;

                    if kcp.is_none() {
                        let udp = SmolUdpSocket::bind("0.0.0.0:0").await?;
                        udp.connect(socket.as_string()).await?;
                        *kcp = Some(kcp::KcpConnector::new(Arc::new(udp), FusoExecutor));
                    }

                    kcp.as_ref().unwrap().connect().await?.into_boxed_stream()
                } else {
                    return Err(SocketErr::NotSupport(socket).into());
                }
            })
        })
    }
}

impl NetSocket for SmolUdpSocket {
    fn local_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::udp(self.smol_udp.local_addr()?)))
    }

    fn peer_addr(&self) -> crate::Result<Address> {
        Ok(Address::One(Socket::udp(self.smol_udp.peer_addr()?)))
    }
}

impl SmolUdpSocket {
    pub async fn bind<A: AsyncToSocketAddrs>(addr: A) -> crate::Result<Self> {
        Ok(Self {
            smol_udp: smol::net::UdpSocket::bind(addr).await?,
            recv_fut: Default::default(),
            recv_from_fut: Default::default(),
            send_fut: Default::default(),
            send_to_fut: Default::default(),
        })
    }

    pub async fn connect<A: AsyncToSocketAddrs>(&self, addr: A) -> crate::Result<()> {
        self.smol_udp.connect(addr).await.map_err(Into::into)
    }
}

impl UdpSocket for SmolUdpSocket
where
    Self: Unpin,
{
    fn poll_recv(
        self: Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> Poll<crate::Result<()>> {
        let mut fut = match self.recv_fut.lock()?.take() {
            Some(fut) => fut,
            None => {
                let smol_udp = self.smol_udp.clone();
                let unfilled = buf.initialize_unfilled();
                let unfilled_len = unfilled.len();
                let unfilled_ptr = unfilled.as_mut_ptr();
                Box::pin(async move {
                    let buf = unsafe {
                        std::ptr::slice_from_raw_parts_mut(unfilled_ptr, unfilled_len)
                            .as_mut()
                            .unwrap_unchecked()
                    };

                    Ok(smol_udp.recv(buf).await?)
                })
            }
        };

        match Pin::new(&mut fut).poll(cx)? {
            Poll::Ready(n) => {
                buf.advance(n);
                Poll::Ready(Ok(()))
            }
            Poll::Pending => {
                let mut locked = self.recv_fut.lock()?;
                drop(std::mem::replace(&mut *locked, Some(fut)));
                Poll::Pending
            }
        }
    }

    fn poll_recv_from(
        self: Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> Poll<crate::Result<SocketAddr>> {
        let mut fut = match self.recv_from_fut.lock()?.take() {
            Some(fut) => fut,
            None => {
                let smol_udp = self.smol_udp.clone();
                let unfilled_buf = buf.initialize_unfilled();
                let unfilled_len = unfilled_buf.len();
                let unfilled_ptr = unfilled_buf.as_mut_ptr();
                Box::pin(async move {
                    let buf = unsafe {
                        std::ptr::slice_from_raw_parts_mut(unfilled_ptr, unfilled_len)
                            .as_mut()
                            .unwrap_unchecked()
                    };
                    smol_udp.recv_from(buf).await.map_err(Into::into)
                })
            }
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(Ok((n, addr))) => {
                buf.advance(n);
                Poll::Ready(Ok(addr))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => {
                let mut locked = self.recv_from_fut.lock()?;
                drop(std::mem::replace(&mut *locked, Some(fut)));
                Poll::Pending
            }
        }
    }

    fn poll_send(
        self: Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        let mut fut = match self.send_fut.lock()?.take() {
            Some(fut) => fut,
            None => {
                let smol_udp = self.smol_udp.clone();
                let buf_ptr = buf.as_ptr();
                let buf_len = buf.len();
                Box::pin(async move {
                    let buf = unsafe {
                        std::ptr::slice_from_raw_parts(buf_ptr, buf_len)
                            .as_ref()
                            .unwrap_unchecked()
                    };
                    smol_udp.send(buf).await.map_err(Into::into)
                })
            }
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(ready) => Poll::Ready(ready),
            Poll::Pending => {
                let mut locked = self.send_fut.lock()?;
                drop(std::mem::replace(&mut *locked, Some(fut)));
                Poll::Pending
            }
        }
    }

    fn poll_send_to(
        self: Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        addr: &SocketAddr,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        let mut fut = match self.send_to_fut.lock()?.take() {
            Some(fut) => fut,
            None => {
                let smol_udp = self.smol_udp.clone();
                let addr = addr as *const SocketAddr;
                let buf_ptr = buf.as_ptr();
                let buf_len = buf.len();
                Box::pin(async move {
                    let (buf, addr) = unsafe {
                        (
                            std::ptr::slice_from_raw_parts(buf_ptr, buf_len)
                                .as_ref()
                                .unwrap_unchecked(),
                            addr.as_ref().unwrap_unchecked(),
                        )
                    };
                    smol_udp.send_to(buf, addr).await.map_err(Into::into)
                })
            }
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(ready) => Poll::Ready(ready),
            Poll::Pending => {
                let mut locked = self.send_to_fut.lock()?;
                drop(std::mem::replace(&mut *locked, Some(fut)));
                Poll::Pending
            }
        }
    }
}

impl Provider<Socket> for FusoUdpServerProvider {
    type Output = BoxedFuture<Arc<SmolUdpSocket>>;
    fn call(&self, socket: Socket) -> Self::Output {
        Box::pin(async move { Ok(Arc::new(SmolUdpSocket::bind(socket.as_string()).await?)) })
    }
}

impl Provider<()> for FusoUdpForwardProvider {
    type Output = BoxedFuture<(SocketAddr, SmolUdpSocket)>;
    fn call(&self, _: ()) -> Self::Output {
        Box::pin(async move {
            let smol_udp = SmolUdpSocket::bind("0.0.0.0:0").await?;
            Ok((smol_udp.smol_udp.local_addr()?, smol_udp))
        })
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

pub fn builder_client() -> client::ClientBuilder<FusoExecutor, FusoConnector, FusoStream> {
    client::ClientBuilder {
        executor: FusoExecutor,
        handshake: None,
        retry_delay: None,
        maximum_retries: None,
        client_provider: ClientProvider {
            server_address: Default::default(),
            connect_provider: Arc::new(FusoConnector(Default::default())),
        },
    }
}
