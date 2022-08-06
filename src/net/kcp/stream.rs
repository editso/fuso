use std::{
    net::SocketAddr,
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::Arc,
    task::{Poll, Waker},
};

use crate::{
    guard::buffer::Buffer, Address, AsyncRead, AsyncWrite, Kind, NetSocket, Task, UdpSocket,
};

use super::KcpErr;

type Callback = Option<Box<dyn FnOnce() + Sync + Send + 'static>>;

#[pin_project::pin_project]
pub struct KOutput<C> {
    #[pin]
    pub(crate) output: C,
    pub(crate) target: Option<SocketAddr>,
}

pub struct KcpCore<C> {
    pub(crate) kcp: super::third_party::Kcp<KOutput<C>>,
    pub(crate) kbuf: Buffer<u8>,
    pub(crate) kupdate: Option<Task<crate::Result<()>>>,
    pub(crate) read_waker: Option<Waker>,
    pub(crate) write_waker: Option<Waker>,
    pub(crate) close_waker: Option<Waker>,
}

pub struct KcpStream<C> {
    pub(crate) conv: u32,
    pub(crate) local_addr: Address,
    pub(crate) peer_addr: Address,
    pub(crate) kcore: Arc<std::sync::Mutex<KcpCore<C>>>,
    /// 在被drop或手动调用close时触发通知内部进行更新
    pub(crate) close_callback: Callback,
}

impl<C> Deref for KcpCore<C> {
    type Target = super::third_party::Kcp<KOutput<C>>;
    fn deref(&self) -> &Self::Target {
        &self.kcp
    }
}

impl<C> DerefMut for KcpCore<C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.kcp
    }
}

impl<C> AsyncWrite for KOutput<C>
where
    C: UdpSocket + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        data: &[u8],
    ) -> Poll<crate::Result<usize>> {
        let this = self.project();
        let output = Pin::new(&*this.output);
        match this.target.as_ref() {
            None => output.poll_send(cx, data),
            Some(addr) => output.poll_send_to(cx, addr, data),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<crate::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<crate::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl<C> AsyncWrite for KcpCore<C>
where
    C: UdpSocket + Unpin + 'static,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<crate::Result<usize>> {
        match self.kcp.send(buf) {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(e) => match e.kind() {
                Kind::Kcp(KcpErr::UserBufTooBig) => {
                    drop(std::mem::replace(
                        &mut self.write_waker,
                        Some(cx.waker().clone()),
                    ));
                    Poll::Pending
                }
                _ => Poll::Ready(Err(e)),
            },
        }
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<()>> {
        if self.kcp.wait_snd() > 0 {
            drop(std::mem::replace(
                &mut self.close_waker,
                Some(cx.waker().clone()),
            ));
            Poll::Pending
        } else {
            self.kcp.force_close()?;
            Poll::Ready(Ok(()))
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut std::task::Context<'_>) -> Poll<crate::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl<C> AsyncRead for KcpCore<C>
where
    C: UdpSocket + Unpin + 'static,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> Poll<crate::Result<usize>> {
        let unfilled = buf.initialize_unfilled();

        if !self.kbuf.is_empty() {
            let n = self.kbuf.read_to_buffer(unfilled);
            buf.advance(n);
            return Poll::Ready(Ok(n));
        }

        match self.kcp.recv(unfilled) {
            Ok(n) => {
                buf.advance(n);
                return Poll::Ready(Ok(n));
            }
            Err(e) => match e.kind() {
                Kind::Kcp(KcpErr::RecvQueueEmpty | KcpErr::ExpectingFragment) => {
                    let waker = cx.waker().clone();
                    drop(std::mem::replace(&mut self.read_waker, Some(waker)));
                    return Poll::Pending;
                }
                Kind::Kcp(KcpErr::UserBufTooSmall) => unsafe {
                    let size = self.kcp.peeksize().unwrap_unchecked();
                    let mut data = Vec::with_capacity(size);

                    data.set_len(size);

                    let n = self.kcp.recv(&mut data)?;
                    data.truncate(n);
                    let len = unfilled.len();

                    std::ptr::copy(data.as_ptr(), unfilled.as_mut_ptr(), len);

                    buf.advance(len);

                    self.kbuf.push_back(&data[len..]);

                    return Poll::Ready(Ok(len));
                },
                _ => Poll::Ready(Err(e)),
            },
        }
    }
}

impl<C> AsyncRead for KcpStream<C>
where
    C: UdpSocket + Unpin + 'static,
{
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        let mut kcore = self.kcore.lock()?;
        Pin::new(&mut *kcore).poll_read(cx, buf)
    }
}

impl<C> AsyncWrite for KcpStream<C>
where
    C: UdpSocket + Unpin + 'static,
{
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        let mut kcore = self.kcore.lock()?;
        Pin::new(&mut *kcore).poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        let mut kcore = self.kcore.lock()?;
        Pin::new(&mut *kcore).poll_flush(cx)
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        if let Some(close_callback) = self.close_callback.take() {
            close_callback();
        }

        Poll::Ready(Ok(()))
    }
}

impl<C> NetSocket for KcpStream<C> {
    fn peer_addr(&self) -> crate::Result<crate::Address> {
        Ok(self.peer_addr.clone())
    }

    fn local_addr(&self) -> crate::Result<crate::Address> {
        Ok(self.local_addr.clone())
    }
}

impl<C> Drop for KcpStream<C> {
    fn drop(&mut self) {
        if let Some(close_callback) = self.close_callback.take() {
            log::debug!("close read and write conv={}", self.conv);
            close_callback();
        } else {
            log::debug!("closed conv {}", self.conv);
        }
    }
}

impl<C> Drop for KcpCore<C> {
    fn drop(&mut self) {
        if let Some(mut kupdate) = self.kupdate.take() {
            log::debug!("abort execution {}", self.kcp);
            kupdate.abort();
        } else {
            log::debug!("kupdate finished");
        }
    }
}
