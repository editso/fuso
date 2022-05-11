use std::task::Poll;

use tokio::net::TcpListener;

use crate::{
    handler::Handler,
    listener::{ext::AccepterExt, Accepter},
    ready,
    server::{builder::FusoServer, server::Server},
    service::{IntoService, IntoServiceFactory},
    AsyncRead, AsyncWrite, Executor,
};

#[derive(Clone, Copy)]
pub struct TokioExecutor;

impl Executor for TokioExecutor {
    fn spawn<F, O>(&self, fut: F)
    where
        F: std::future::Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        tokio::spawn(fut);
    }
}

impl<S> FusoServer<S, TokioExecutor>
where
    S: AsyncWrite + AsyncRead + Unpin + Send + 'static,
{
    pub fn new() -> Self {
        Self {
            executor: TokioExecutor,
            services: Default::default(),
        }
    }
}

impl Accepter for TcpListener {
    type Stream = tokio::net::TcpStream;

    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        let (tcp, addr) = ready!(self.get_mut().poll_accept(cx)?);
        Poll::Ready(Ok(tcp))
    }
}
