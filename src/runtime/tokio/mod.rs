use std::{pin::Pin, sync::Arc, task::Poll};

use tokio::net::TcpListener;

use crate::{
    listener::Accepter,
    service::{Factory, ServerFactory, Transfer},
    Addr, FusoStream, Executor,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

#[derive(Clone, Copy)]
pub struct TokioExecutor;

pub struct TokioTcpListener(tokio::net::TcpListener);
pub struct TokioAccepter;
pub struct TokioConnector;

impl Executor for TokioExecutor {
    fn spawn<F, O>(&self, fut: F)
    where
        F: std::future::Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        tokio::spawn(fut);
    }
}

impl Factory<Addr> for TokioAccepter {
    type Output = BoxedFuture<TokioTcpListener>;

    fn call(&self, cfg: Addr) -> Self::Output {
        Box::pin(async move {
            match cfg {
                Addr::Socket(addr) => TcpListener::bind(addr)
                    .await
                    .map_err(Into::into)
                    .map(|tcp| TokioTcpListener(tcp)),
                Addr::Domain(_, _) => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "invalid socket address",
                )
                .into()),
            }
        })
    }
}

impl Accepter for TokioTcpListener {
    type Stream = tokio::net::TcpStream;
    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<Self::Stream>> {
        match self.0.poll_accept(cx) {
            Poll::Ready(Ok((tcp, _))) => Poll::Ready(Ok(tcp)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e.into())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Factory<Addr> for TokioConnector {
    type Output = BoxedFuture<tokio::net::TcpStream>;

    fn call(&self, cfg: Addr) -> Self::Output {
        Box::pin(async move {
            tokio::net::TcpStream::connect(format!("{}", cfg))
                .await
                .map_err(Into::into)
        })
    }
}

impl ServerFactory<TokioAccepter, TokioConnector> {
    pub fn with_tokio() -> Self {
        ServerFactory {
            accepter_factory: Arc::new(TokioAccepter),
            connector_factory: Arc::new(TokioConnector),
        }
    }
}

impl From<tokio::net::TcpStream> for FusoStream {
    fn from(t: tokio::net::TcpStream) -> Self {
        Self::new(t)
    }
}


