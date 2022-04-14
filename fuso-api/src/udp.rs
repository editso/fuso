use std::{
    collections::HashMap,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Poll, Waker},
};

use futures::{AsyncRead, AsyncWrite};
use smol::{
    channel::{unbounded, Receiver, Sender},
    future::FutureExt,
    io::AsyncWriteExt,
    net::{AsyncToSocketAddrs, UdpSocket},
    Task,
};

use std::sync::Mutex;

use crate::{Buffer, Spawn};

pub const MTU_BUF_SIZE: usize = 1400;

pub struct UdpListener {
    addr: SocketAddr,
    accept_ax: Receiver<UdpStream>,
    core: Arc<Mutex<Option<Task<()>>>>,
}

pub struct UdpStream {
    inner: UdpSocket,
    store: Buffer<u8>,
    closed: Arc<Mutex<bool>>,
    core: Arc<Mutex<Option<Task<()>>>>,
    read_waker: Arc<Mutex<Option<Waker>>>,
    addr: SocketAddr,
    is_server: bool,
}

pub trait UdpStreamEx {
    fn as_udp_stream(self) -> UdpStream;
}

impl UdpStream {
    pub fn new(
        udp: UdpSocket,
        server_stream: Option<(smol::channel::Receiver<Vec<u8>>, SocketAddr)>,
    ) -> Self {
        let closed = Arc::new(Mutex::new(false));
        let mut store = Buffer::new();

        let (server_stream, addr, is_server) = server_stream
            .map(|(server, addr)| (Some(server), addr, true))
            .unwrap_or((None, udp.local_addr().unwrap(), false));

        let read_waker = Arc::new(Mutex::new(None));

        Self {
            inner: udp.clone(),
            store: store.clone(),
            closed: closed.clone(),
            read_waker: read_waker.clone(),
            addr: addr,
            is_server,
            core: {
                let read_future = async move {
                    loop {
                        let packet = if let Some(receiver) = server_stream.as_ref() {
                            let packet = receiver.recv().await;

                            if packet.is_err() {
                                log::debug!("udp error {}", packet.unwrap_err());
                                break;
                            }
                            packet.unwrap()
                        } else {
                            let mut buf = Vec::with_capacity(MTU_BUF_SIZE);

                            unsafe {
                                buf.set_len(MTU_BUF_SIZE);
                            }

                            let packet = udp.recv(&mut buf).await;
                            if packet.is_err() {
                                break;
                            }

                            let n = packet.unwrap();
                            buf.truncate(n);

                            buf
                        };

                        let _ = store.write_all(&packet).await;

                        if let Some(waker) = read_waker.lock().unwrap().take() {
                            waker.wake();
                        }
                    }
                };

                Arc::new(Mutex::new(Some(smol::spawn(read_future))))
            },
        }
    }
}

impl UdpListener {
    pub async fn bind<A: AsyncToSocketAddrs>(addr: A) -> std::io::Result<Self> {
        let udp = UdpSocket::bind(addr).await?;
        let (accept_tx, accept_ax) = unbounded();

        let sessions: Arc<smol::lock::Mutex<HashMap<String, Sender<Vec<u8>>>>> =
            Arc::new(smol::lock::Mutex::new(HashMap::new()));

        let addr = udp.local_addr().unwrap();

        let core_future = async move {
            loop {
                let mut buf = Vec::with_capacity(MTU_BUF_SIZE);

                unsafe {
                    buf.set_len(MTU_BUF_SIZE);
                }

                let packet = udp.recv_from(&mut buf).await;

                if packet.is_err() {
                    log::error!("udp server error {}", packet.unwrap_err());
                    break;
                }

                let (n, addr) = packet.unwrap();

                buf.truncate(n);

                let mut sessions = sessions.lock().await;

                let core = sessions.entry(addr.to_string()).or_insert({
                    let (sender, receiver) = unbounded();

                    let stream = UdpStream::new(udp.clone(), Some((receiver, addr)));

                    if let Err(_) = accept_tx.send(stream).await {
                        return;
                    }
                    sender
                });

                if let Err(_) = core.send(buf).await {
                    sessions.retain(|_, core| !core.is_closed());
                }
            }
        };

        Ok(Self {
            addr,
            accept_ax,
            core: Arc::new(Mutex::new(Some(smol::spawn(core_future)))),
        })
    }

    #[inline]
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    #[inline]
    pub async fn accept(&self) -> std::io::Result<UdpStream> {
        self.accept_ax
            .recv()
            .await
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

impl Drop for UdpListener {
    #[inline]
    fn drop(&mut self) {
        if let Some(core) = self.core.lock().unwrap().take() {
            core.cancel().detach()
        }
    }
}

impl UdpStreamEx for UdpSocket {
    #[inline]
    fn as_udp_stream(self) -> UdpStream {
        UdpStream::new(self, None)
    }
}

impl UdpStream {
    #[inline]
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        Ok(self.addr)
    }
}

impl AsyncRead for UdpStream {
    #[inline]
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if self.closed.lock().unwrap().eq(&true) {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "Connection has been closed",
            )))
        } else {
            match Pin::new(&mut self.store).poll_read(cx, buf) {
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Ready(Ok(0)) => {
                    *self.read_waker.lock().unwrap() = Some(cx.waker().clone());
                    Poll::Pending
                }
                Poll::Ready(Ok(n)) => Poll::Ready(Ok(n)),
                Poll::Pending => todo!(),
            }
        }
    }
}

impl AsyncWrite for UdpStream {
    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.closed.lock().unwrap().eq(&true) {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "Connection has been closed",
            )))
        } else {
            let udp = self.inner.clone();
            let addr = self.addr.clone();
            let is_server = self.is_server.clone();

            Box::pin(async move {
                if is_server {
                    udp.send_to(buf, addr).await
                } else {
                    udp.send(buf).await
                }
            })
            .poll(cx)
        }
    }

    #[inline]
    fn poll_flush(
        self: Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    #[inline]
    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Some(core) = self.core.lock().unwrap().take() {
            let _ = Box::pin(core.cancel()).poll(cx);
        }

        *self.closed.lock().unwrap() = true;

        Poll::Ready(Ok(()))
    }
}
