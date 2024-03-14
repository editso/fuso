use std::{
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use crate::{
    core::{
        accepter::Accepter,
        future::StoredFuture,
        io::{AsyncRead, AsyncWrite},
    },
    error,
};

use super::{UdpProvider, UdpSocket};

pub trait KcpProvider: UdpProvider {
    type Runtime: kcp_rust::KcpRuntime<Err = error::FusoError>;
}

#[pin_project::pin_project]
pub struct KcpListener {
    #[pin]
    pub(crate) inner: Arc<kcp_rust::KcpListener<UdpSocket<'static>>>,
    pub(crate) stored: StoredFuture<
        'static,
        std::io::Result<(SocketAddr, kcp_rust::KcpStream<kcp_rust::ServerImpl>)>,
    >,
}

pub enum KcpStream {
    Client(kcp_rust::KcpStream<kcp_rust::ClientImpl>),
    Server(kcp_rust::KcpStream<kcp_rust::ServerImpl>),
}

#[cfg(feature = "fuso-runtime")]
impl KcpListener {
    pub async fn bind_with_provider<P, A>(conf: kcp_rust::Config, addr: A) -> error::Result<Self>
    where
        P: KcpProvider,
        A: Into<SocketAddr>,
    {
        let udp = UdpSocket::bind::<P, _>(addr).await?;

        let listener = kcp_rust::KcpListener::new::<P::Runtime>(udp, conf)?;

        Ok(KcpListener {
            inner: Arc::new(listener),
            stored: StoredFuture::new(),
        })
    }
}

impl Accepter for KcpListener {
    type Output = (SocketAddr, KcpStream);

    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<Self::Output>> {
        let this = self.project();
        let poll = this.stored.poll(ctx, || {
            let listener = this.inner.clone();
            async move {
                let (_, addr, stream) = listener.accept().await?;
                Ok((addr, stream))
            }
        })?;

        match poll {
            Poll::Pending => Poll::Pending,
            Poll::Ready((addr, stream)) => Poll::Ready(Ok((addr, KcpStream::Server(stream)))),
        }
    }
}

impl AsyncRead for KcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<error::Result<usize>> {
        match &mut *self {
            KcpStream::Client(s) => {
                kcp_rust::AsyncRead::poll_read(Pin::new(s), cx, buf).map_err(Into::into)
            }
            KcpStream::Server(s) => {
                kcp_rust::AsyncRead::poll_read(Pin::new(s), cx, buf).map_err(Into::into)
            }
        }
    }
}

impl AsyncWrite for KcpStream {
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<error::Result<()>> {
        match &mut *self {
            KcpStream::Client(s) => {
                kcp_rust::AsyncWrite::poll_flush(Pin::new(s), cx).map_err(Into::into)
            }
            KcpStream::Server(s) => {
                kcp_rust::AsyncWrite::poll_flush(Pin::new(s), cx).map_err(Into::into)
            }
        }
    }

    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<error::Result<usize>> {
        match &mut *self {
            KcpStream::Client(s) => {
                kcp_rust::AsyncWrite::poll_write(Pin::new(s), cx, buf).map_err(Into::into)
            }
            KcpStream::Server(s) => {
                kcp_rust::AsyncWrite::poll_write(Pin::new(s), cx, buf).map_err(Into::into)
            }
        }
    }
}
