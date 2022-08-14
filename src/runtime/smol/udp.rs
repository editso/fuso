use std::{pin::Pin, net::SocketAddr, task::Poll};

use futures::Future;
use smol::net::AsyncToSocketAddrs;

use crate::{NetSocket, Address, Socket, UdpSocket};

type UFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + 'static>>;

pub struct SmolUdpSocket {
    smol_udp: smol::net::UdpSocket,
    recv_fut: std::sync::Mutex<Option<UFuture<usize>>>,
    recv_from_fut: std::sync::Mutex<Option<UFuture<(usize, std::net::SocketAddr)>>>,
    send_fut: std::sync::Mutex<Option<UFuture<usize>>>,
    send_to_fut: std::sync::Mutex<Option<UFuture<usize>>>,
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
    pub async fn bind<A: AsyncToSocketAddrs>(addr: A) -> crate::Result<(SocketAddr, Self)> {
        let udp_server = smol::net::UdpSocket::bind(addr).await?;
        let listen_addr = udp_server.local_addr()?;
        Ok((
            listen_addr,
            Self {
                smol_udp: udp_server,
                recv_fut: Default::default(),
                recv_from_fut: Default::default(),
                send_fut: Default::default(),
                send_to_fut: Default::default(),
            },
        ))
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
