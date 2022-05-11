pub mod tun;

use std::{
    net::SocketAddr,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::Poll,
};

use futures::{Future, FutureExt};
use smol::net::{AsyncToSocketAddrs, Incoming, TcpStream};

use crate::{listener::Accepter, server::builder::FusoServer, AsyncRead, AsyncWrite, Executor};

#[derive(Default, Clone)]
pub struct SmolExecutor;

pub struct TcpListener {
    inner: smol::net::TcpListener,
    fut: Option<Pin<Box<dyn Future<Output = std::io::Result<TcpStream>> + Send + 'static>>>,
}

impl TcpListener {
    pub async fn bind<A>(addr: A) -> std::io::Result<Self>
    where
        A: AsyncToSocketAddrs,
    {
        let tcp = smol::net::TcpListener::bind(addr).await?;
        Ok(Self {
            inner: tcp,
            fut: None,
        })
    }
}

impl Executor for SmolExecutor {
    fn spawn<F, O>(&self, fut: F)
    where
        F: futures::Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        smol::spawn(fut).detach();
    }
}

impl<S> FusoServer<S, SmolExecutor>
where
    S: AsyncWrite + AsyncRead + Unpin + Send + 'static,
{
    pub fn new() -> Self {
        Self {
            executor: SmolExecutor,
            services: Default::default(),
        }
    }
}

impl<'a> Accepter for TcpListener {
    type Stream = smol::net::TcpStream;

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        let mut fut = if let Some(fut) = self.fut.take() {
            fut
        } else {
            let tcp = self.inner.clone();
            Box::pin(async move {
                let tcp = tcp.accept().await;
                match tcp {
                    Ok((tcp, _)) => Ok(tcp),
                    Err(e) => Err(e),
                }
            })
        };

        match fut.poll_unpin(cx)? {
            Poll::Ready(r) => Poll::Ready(Ok(r)),
            Poll::Pending => {
                drop(std::mem::replace(&mut self.fut, Some(fut)));
                Poll::Pending
            }
        }
    }
}

impl Deref for TcpListener {
    type Target = smol::net::TcpListener;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for TcpListener {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
