use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Poll, Waker},
};

use async_mutex::Mutex;

use crate::{guard::buffer::Buffer, AsyncRead, AsyncWrite, UdpSocket};

use super::KcpErr;

type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;
type Callback = Option<Box<dyn Fn() + Sync + Send + 'static>>;

#[pin_project::pin_project]
pub struct KOutput<C> {
    #[pin]
    pub(crate) output: C,
    pub(crate) target: Option<SocketAddr>,
}

pub struct KcpCore<C> {
    pub(crate) kcp: Arc<Mutex<(Buffer<u8>, super::third_party::Kcp<KOutput<C>>)>>,
    pub(crate) read_waker: Option<Waker>,
    pub(crate) write_waker: Option<Waker>,
    pub(crate) close_waker: Option<Waker>,
}

pub struct KcpStream<C> {
    pub(crate) conv: u32,
    pub(crate) kcore: KcpCore<C>,
    /// 在被drop或手动调用close时触发通知内部进行更新
    pub(crate) close_callback: Callback,
    /// 在每次调用 read, write, flush更新kcp状态
    pub(crate) lazy_kcp_update: std::sync::Mutex<Option<BoxedFuture<crate::Result<()>>>>,
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

impl<C> AsyncRead for KcpStream<C>
where
    C: UdpSocket + Unpin + 'static,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        let mut kcp_update = self.lazy_kcp_update.lock()?;
        match kcp_update.take() {
            None => return Poll::Ready(Err(KcpErr::Closed.into())),
            Some(mut kcp) => match Pin::new(&mut kcp).poll(cx)? {
                Poll::Ready(()) => return Poll::Ready(Ok(0)),
                Poll::Pending => {
                    drop(std::mem::replace(&mut *kcp_update, Some(kcp)));
                    drop(kcp_update);
                    Pin::new(&mut self.kcore).poll_read(cx, buf)
                }
            },
        }
    }
}

impl<C> AsyncWrite for KcpStream<C>
where
    C: UdpSocket + Unpin + 'static,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        let mut kcp_update = self.lazy_kcp_update.lock()?;
        match kcp_update.take() {
            None => Poll::Ready(Err(KcpErr::Closed.into())),
            Some(mut kcp) => match Pin::new(&mut kcp).poll(cx)? {
                Poll::Ready(()) => return Poll::Ready(Ok(0)),
                Poll::Pending => {
                    drop(std::mem::replace(&mut *kcp_update, Some(kcp)));
                    drop(kcp_update);
                    Pin::new(&mut self.kcore).poll_write(cx, buf)
                }
            },
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        let mut kcp_update = self.lazy_kcp_update.lock()?;
        match kcp_update.take() {
            None => return Poll::Ready(Err(KcpErr::Closed.into())),
            Some(mut kcp) => match Pin::new(&mut kcp).poll(cx)? {
                Poll::Ready(()) => return Poll::Ready(Ok(())),
                Poll::Pending => {
                    drop(std::mem::replace(&mut *kcp_update, Some(kcp)));
                    drop(kcp_update);
                    Pin::new(&mut self.kcore).poll_flush(cx)
                }
            },
        }
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        let mut kcp_checker = self.lazy_kcp_update.lock()?;
        match kcp_checker.take() {
            None => return Poll::Ready(Err(KcpErr::Closed.into())),
            Some(mut kcp) => match Pin::new(&mut kcp).poll(cx)? {
                Poll::Ready(()) => return Poll::Ready(Ok(())),
                Poll::Pending => {
                    std::mem::replace(&mut *kcp_checker, Some(kcp));
                    drop(kcp_checker);
                    Pin::new(&mut self.kcore).poll_close(cx)
                }
            },
        }
    }
}

impl<C> Drop for KcpStream<C> {
    fn drop(&mut self) {
        if let Some(close_callback) = self.close_callback.take() {
            log::debug!("close read and write conv={}", self.conv);
            close_callback()
        }
    }
}
