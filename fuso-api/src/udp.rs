use std::{
    collections::HashMap,
    net::SocketAddr,
    pin::Pin,
    sync::{
        mpsc::{sync_channel, SyncSender},
        Arc,
    },
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

use crate::{Buffer, Spwan};

pub struct UdpListener {
    accept_ax: Receiver<UdpStream>,
    core: Arc<Mutex<Option<Task<()>>>>,
}

pub struct UdpStream {
    inner: Arc<Mutex<UdpSocket>>,
    store: Arc<Mutex<Buffer<u8>>>,
    sender: Option<SyncSender<Vec<u8>>>,
    closed: Arc<Mutex<bool>>,
    core: Arc<Mutex<Option<Task<()>>>>,
    read_waker: Arc<Mutex<Option<Waker>>>,
    send_waker: Arc<Mutex<Option<Waker>>>,
    addr: SocketAddr,
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
        let store = Arc::new(Mutex::new(Buffer::new()));
        let (sender, send_receiver) = sync_channel(10);

        let send_receiver = Arc::new(Mutex::new(send_receiver));

        let (server_stream, addr, is_server) = server_stream
            .map(|(server, addr)| (Some(server), addr, true))
            .unwrap_or((None, udp.local_addr().unwrap(), false));

        let read_waker = Arc::new(Mutex::new(None));
        let send_waker = Arc::new(Mutex::new(None));

        Self {
            inner: Arc::new(Mutex::new(udp.clone())),
            store: store.clone(),
            sender: Some(sender),
            closed: closed.clone(),
            read_waker: read_waker.clone(),
            send_waker: send_waker.clone(),
            addr: addr,
            core: {
                let write_future = {
                    let closed = closed.clone();
                    let writer = udp.clone();
                    async move {
                        loop {
                            let receiver = send_receiver.clone();
                            let send_waker = send_waker.clone();

                            let packet = smol::future::poll_fn(move |cx| {
                                receiver.lock().unwrap().try_recv().map_or_else(
                                    |e| match e {
                                        std::sync::mpsc::TryRecvError::Empty => {
                                            *send_waker.lock().unwrap() = Some(cx.waker().clone());
                                            Poll::Pending
                                        }
                                        std::sync::mpsc::TryRecvError::Disconnected => Poll::Ready(
                                            Err(std::sync::mpsc::TryRecvError::Disconnected),
                                        ),
                                    },
                                    |packet| Poll::Ready(Ok(packet)),
                                )
                            })
                            .await;

                            if packet.is_err() {
                                *closed.lock().unwrap() = true;
                                break;
                            }

                            let buf = packet.unwrap();

                            log::debug!("[udp] send total {}", buf.len());

                            for packet in buf.chunks(1350).into_iter() {
                                if let Err(e) = if is_server {
                                    writer.send_to(&packet, addr).await
                                } else {
                                    writer.send(&packet).await
                                } {
                                    log::debug!("udp error {}", e);
                                    *closed.lock().unwrap() = true;
                                    return;
                                };
                            }
                        }
                    }
                };

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
                            let mut buf = Vec::new();
                            buf.resize(1350, 0);

                            let packet = udp.recv(&mut buf).await;
                            if packet.is_err() {
                                break;
                            }

                            buf
                        };

                        let mut store = store.lock().unwrap().clone();
                        let _ = store.write_all(&packet).await;

                        if let Some(waker) = read_waker.lock().unwrap().take() {
                            waker.wake();
                        }
                    }
                };

                Arc::new(Mutex::new(Some(smol::spawn(
                    read_future.race(write_future),
                ))))
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

        let core_future = async move {
            loop {
                let mut buf = Vec::new();

                buf.resize(1350, 0);

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
            accept_ax,
            core: Arc::new(Mutex::new(Some(smol::spawn(core_future)))),
        })
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
        self.inner.lock().unwrap().local_addr()
    }

    #[inline]
    pub fn peer_addr(&self) -> std::io::Result<SocketAddr> {
        Ok(self.addr)
    }
}

impl AsyncRead for UdpStream {
    #[inline]
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        if self.closed.lock().unwrap().eq(&true) {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "Connection has been closed",
            )))
        } else {
            let mut udp = self.store.lock().unwrap();
            match Pin::new(&mut *udp).poll_read(cx, buf) {
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
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.closed.lock().unwrap().eq(&true) {
            Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "Connection has been closed",
            )))
        } else {
            let len = buf.len();

            if let Some(sender) = self.sender.as_ref() {
                if let Err(_) = sender.send(buf.to_vec()) {
                    *self.closed.lock().unwrap() = false;
                    Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "Connection has been closed",
                    )))
                } else {
                    if let Some(waker) = self.send_waker.lock().unwrap().take() {
                        waker.wake();
                    }

                    Poll::Ready(Ok(len))
                }
            } else {
                Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "Connection has been closed",
                )))
            }
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
        mut self: Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        if let Some(core) = self.core.lock().unwrap().take() {
            core.cancel().detach()
        }

        if let Some(sender) = self.sender.take() {
            drop(sender)
        }

        *self.closed.lock().unwrap() = true;

        Poll::Ready(Ok(()))
    }
}
