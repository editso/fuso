use std::{net::SocketAddr, sync::Arc, task::Poll};

use tokio::io::ReadBuf;

use crate::{
    core::{
        future::StoredFuture,
        net::{BoxedDatagram, KcpListener},
        BoxedFuture,
    },
    error,
    runtime::Runtime,
};

pub struct KcpWithTokioRuntime;

pub struct TokioUdpSocket(Arc<tokio::net::UdpSocket>);

impl KcpListener {
    pub async fn bind_with_tokio<A>(conf: kcp_rust::Config, addr: A) -> error::Result<Self>
    where
        A: Into<SocketAddr>,
    {
        let udp = crate::core::net::UdpSocket::bind::<KcpWithTokioRuntime, _>(addr).await?;
        let listener = kcp_rust::KcpListener::new::<KcpWithTokioRuntime>(udp, conf)?;
        Ok(Self {
            inner: Arc::new(listener),
            stored: StoredFuture::new(),
        })
    }
}

impl crate::core::net::UdpProvider for KcpWithTokioRuntime {
    type Binder = Self;

    type Connect = Self;
}

impl crate::core::Provider<BoxedFuture<'static, error::Result<BoxedDatagram<'static>>>>
    for KcpWithTokioRuntime
{
    type Arg = SocketAddr;

    fn call(addr: Self::Arg) -> BoxedFuture<'static, error::Result<BoxedDatagram<'static>>> {
        Box::pin(async move {
            let boxed: BoxedDatagram<'_> = Box::new(TokioUdpSocket(Arc::new({
                tokio::net::UdpSocket::bind(addr).await?
            })));
            Ok(boxed)
        })
    }
}

impl kcp_rust::KcpRuntime for KcpWithTokioRuntime {
    type Err = error::FusoError;

    type Runner = Self;

    type Timer = Self;

    fn timer() -> Self::Timer {
        Self
    }
}

impl kcp_rust::Runner for KcpWithTokioRuntime {
    type Err = error::FusoError;

    fn start(process: kcp_rust::Background) -> std::result::Result<(), Self::Err> {
        log::debug!("starting {:?}", process.kind());
        crate::runtime::tokio::TokioRuntime::spawn(async move{
          process.await.unwrap();
          log::debug!("kcp finished ....")
        });
        Ok(())
    }
}

impl kcp_rust::Timer for KcpWithTokioRuntime {
    type Ret = ();

    type Output = BoxedFuture<'static, Self::Ret>;

    fn sleep(&self, time: std::time::Duration) -> Self::Output {
        Box::pin(async move {
            let _ = tokio::time::sleep(time).await;
        })
    }
}

impl crate::core::net::Datagram for TokioUdpSocket {
    fn boxed_clone(&self) -> Box<dyn crate::core::net::Datagram + Sync + Send + Unpin> {
        Box::new(TokioUdpSocket(self.0.clone()))
    }
}

impl crate::core::net::AsyncRecvfrom for TokioUdpSocket {
    fn poll_recvfrom(
        &mut self,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<(std::net::SocketAddr, usize)>> {
        let mut buf = ReadBuf::new(buf);
        match self.0.poll_recv_from(cx, &mut buf)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(addr) => Poll::Ready(Ok((addr, buf.filled().len()))),
        }
    }
}

impl crate::core::net::AsyncSendTo for TokioUdpSocket {
    fn poll_sendto(
        &mut self,
        cx: &mut std::task::Context<'_>,
        addr: &std::net::SocketAddr,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.0.poll_send_to(cx, buf, *addr)
    }
}

impl crate::core::net::AsyncRecv for TokioUdpSocket {
    fn poll_recv(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let mut buf = ReadBuf::new(buf);
        match self.0.poll_recv(cx, &mut buf)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready(()) => Poll::Ready(Ok(buf.filled().len())),
        }
    }
}

impl crate::core::net::AsyncSend for TokioUdpSocket {
    fn poll_send(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.0.poll_send(cx, buf)
    }
}
