use std::{net::SocketAddr, task::Poll};

use crate::{traits::Executor, Accepter, Register};

#[derive(Default, Clone)]
pub struct DefaultExecutor;
pub struct DefaultRegister;

#[derive(Default, Clone)]
pub struct DefaultListener;

impl Executor for DefaultExecutor {
    type Output = ();
    fn spawn<F>(&self, fut: F) -> Self::Output
    where
        F: std::future::Future<Output = Self::Output> + Send + 'static,
    {
        tokio::spawn(fut);
        ()
    }
}

impl Register for DefaultRegister {
    type Output = SocketAddr;
    type Metadata = DefaultListener;

    fn poll_register(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        metadata: &Self::Metadata,
    ) -> Poll<crate::Result<Self::Output>> {
        unimplemented!()
    }
}

impl Accepter for DefaultListener {
    type Stream = tokio::net::TcpStream;

    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<Self::Stream>> {
        unimplemented!()
    }
}
