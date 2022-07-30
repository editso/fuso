use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use async_mutex::Mutex;

use crate::{
    guard::buffer::Buffer, Address, Executor, NetSocket, ReadBuf, Socket, Task, UdpReceiverExt,
    UdpSocket,
};

type UCore = Arc<std::sync::Mutex<Session>>;
type USessions = Arc<Mutex<HashMap<u64, UCore>>>;
type CloseCallback = Option<Box<dyn FnOnce()>>;

#[derive(Debug, Default)]
struct Session {
    ubuf: Buffer<u8>,
    uwaker: Option<Waker>,
}

pub struct Datagram<U, E> {
    executor: E,
    udp_socket: U,
    udp_sessions: USessions,
    udp_receiver: Option<Task<crate::Result<()>>>,
}

pub struct VirtualUdpSocket<U> {
    ucore: UCore,
    peer_addr: SocketAddr,
    udp_socket: U,
    close_callback: CloseCallback,
}

impl Session {
    fn input(&mut self, data: Vec<u8>) {
        self.ubuf.push_all(data);
        if let Some(waker) = self.uwaker.take() {
            waker.wake();
        }
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<crate::Result<()>> {
        if self.ubuf.is_empty() {
            drop(std::mem::replace(
                &mut self.uwaker,
                Some(cx.waker().clone()),
            ));
            Poll::Pending
        } else {
            let unfilled = buf.initialize_unfilled();
            let n = self.ubuf.read_to_buffer(unfilled);
            buf.advance(n);
            Poll::Ready(Ok(()))
        }
    }
}

impl<U, E> Datagram<U, E>
where
    U: UdpSocket + Sync + Unpin + Clone + Send + 'static,
    E: Executor + Clone + Send + 'static,
{
    pub fn new(udp: U, executor: E) -> crate::Result<Self> {
        let sessions: USessions = Default::default();

        let task = {
            let udp = udp.clone();
            let sessions = sessions.clone();
            executor.spawn(async move {
                loop {
                    let mut buf = Vec::with_capacity(1500);
                    unsafe {
                        buf.set_len(1500);
                    }

                    let (n, addr) = udp.recv_from(&mut buf).await?;
                    buf.truncate(n);

                    log::trace!("receiver from {} {}bytes", addr, n);

                    let id = {
                        let mut hasher = DefaultHasher::default();
                        addr.hash(&mut hasher);
                        hasher.finish()
                    };

                    let mut sessions = sessions.lock().await;
                    match sessions.get_mut(&id) {
                        Some(session) => session.lock()?.input(buf),
                        None => {
                            log::debug!("bad session {}", id);
                        }
                    }
                }
            })
        };

        Ok(Self {
            executor,
            udp_socket: udp,
            udp_sessions: sessions,
            udp_receiver: Some(task),
        })
    }

    pub async fn connect(&self, addr: SocketAddr) -> crate::Result<VirtualUdpSocket<U>> {
        let ucore: UCore = Default::default();
        let executor = self.executor.clone();
        let sessions = self.udp_sessions.clone();

        let id = {
            let mut hasher = DefaultHasher::default();
            addr.hash(&mut hasher);
            hasher.finish()
        };

        self.udp_sessions.lock().await.insert(id, ucore.clone());

        Ok(VirtualUdpSocket {
            ucore,
            peer_addr: addr,
            udp_socket: self.udp_socket.clone(),
            close_callback: Some(Box::new(move || {
                executor.spawn(async move {
                    if let Some(session) = sessions.lock().await.remove(&id) {
                        drop(session)
                    };
                });
            })),
        })
    }
}

impl<U> NetSocket for VirtualUdpSocket<U>
where
    U: UdpSocket,
{
    fn local_addr(&self) -> crate::Result<crate::Address> {
        self.udp_socket.local_addr()
    }

    fn peer_addr(&self) -> crate::Result<crate::Address> {
        Ok(Address::One(Socket::udp(self.peer_addr.clone())))
    }
}

impl<U> Drop for VirtualUdpSocket<U> {
    fn drop(&mut self) {
        if let Some(close_callback) = self.close_callback.take() {
            close_callback()
        }
    }
}

impl<U, E> Drop for Datagram<U, E> {
    fn drop(&mut self) {
        if let Some(mut receiver) = self.udp_receiver.take() {
            receiver.abort();
        }
    }
}

unsafe impl<U> Sync for VirtualUdpSocket<U> {}

unsafe impl<U> Send for VirtualUdpSocket<U> {}

impl<U> UdpSocket for VirtualUdpSocket<U>
where
    U: UdpSocket + Unpin + Send + 'static,
{
    fn poll_recv_from(
        self: std::pin::Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<std::net::SocketAddr>> {
        let mut ucore = self.ucore.lock()?;
        match Pin::new(&mut *ucore).poll_recv(cx, buf)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(()) => Poll::Ready(Ok(self.peer_addr.clone())),
        }
    }

    fn poll_send(
        self: std::pin::Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        Pin::new(&self.udp_socket).poll_send_to(cx, &self.peer_addr, buf)
    }

    fn poll_recv(
        self: std::pin::Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        let mut ucore = self.ucore.lock()?;
        Pin::new(&mut *ucore).poll_recv(cx, buf)
    }

    fn poll_send_to(
        self: std::pin::Pin<&Self>,
        cx: &mut std::task::Context<'_>,
        _: &std::net::SocketAddr,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        self.poll_send(cx, buf)
    }
}
