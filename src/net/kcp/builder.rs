use std::{future::Future, pin::Pin, task::Poll};

use crate::{
    mixing::MixListener, server::ServerBuilder, Accepter, Factory, FactoryWrapper, FusoStream,
    Socket, Transfer, UdpSocket,
};

use super::KcpListener;

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct KcpAccepterFactory<C>(FactoryWrapper<Socket, C>);

pub struct KcpAccepter<C>(KcpListener<C>);

impl<C> Accepter for KcpAccepter<C>
where
    C: UdpSocket + Clone + Sync + Unpin + Send + 'static,
{
    type Stream = FusoStream;

    fn poll_accept(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<Self::Stream>> {
        Pin::new(&mut self.0)
            .poll_accept(cx)?
            .map(|kcp| Ok(kcp.transfer()))
    }
}

impl<E, SF, CF, A1> ServerBuilder<E, SF, CF, FusoStream>
where
    SF: Factory<Socket, Output = BoxedFuture<A1>> + Send + Sync + 'static,
    A1: Accepter<Stream = FusoStream> + Unpin + Send + 'static,
{
    pub fn with_kcp_accepter<F, U>(
        self,
        factory: F,
    ) -> ServerBuilder<E, MixListener<SF, KcpAccepterFactory<U>, FusoStream>, CF, FusoStream>
    where
        F: Factory<Socket, Output = BoxedFuture<U>> + Send + Sync + 'static,
        U: UdpSocket + Clone + Sync + Unpin + Send + 'static,
    {
        self.add_accepter(KcpAccepterFactory(FactoryWrapper::wrap(factory)))
    }
}

impl<C> Factory<Socket> for KcpAccepterFactory<C>
where
    C: UdpSocket + Clone + Sync + Unpin + Send + 'static,
{
    type Output = BoxedFuture<KcpAccepter<C>>;

    fn call(&self, arg: Socket) -> Self::Output {
        let fut = self.0.call(arg);
        Box::pin(async move { Ok(KcpAccepter(KcpListener::bind(fut.await?)?)) })
    }
}
