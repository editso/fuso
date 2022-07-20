use std::{future::Future, pin::Pin, task::Poll};

use crate::{
    mixing::MixListener, server::ServerBuilder, Accepter, Factory, FactoryWrapper, FusoStream,
    Socket, Transfer, UdpSocket, Executor,
};

use super::KcpListener;

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct KcpAccepterFactory<C, E>{
    factory: FactoryWrapper<Socket, C>,
    executor: E
}

pub struct KcpAccepter<C, E>(KcpListener<C, E>);

impl<C, E> Accepter for KcpAccepter<C, E>
where
    C: UdpSocket + Clone + Sync + Unpin + Send + 'static,
    E: Executor + Clone + Send + Unpin + 'static
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
    E: Executor + Clone + Sync + Send + Unpin + 'static,
{
    pub fn with_kcp_accepter<F, U>(
        self,
        factory: F,
        executor: E,
    ) -> ServerBuilder<E, MixListener<SF, KcpAccepterFactory<U, E>, FusoStream>, CF, FusoStream>
    where
        F: Factory<Socket, Output = BoxedFuture<U>> + Send + Sync + 'static,
        U: UdpSocket + Clone + Sync + Unpin + Send + 'static,
    {
        self.add_accepter(KcpAccepterFactory{
            executor,
            factory: FactoryWrapper::wrap(factory),
        })
    }
}

impl<C, E> Factory<Socket> for KcpAccepterFactory<C, E>
where
    C: UdpSocket + Clone + Sync + Unpin + Send + 'static,
    E: Executor + Clone + Send + 'static
{
    type Output = BoxedFuture<KcpAccepter<C, E>>;

    fn call(&self, arg: Socket) -> Self::Output {
        let fut = self.factory.call(arg);
        let executor = self.executor.clone();
        Box::pin(async move { Ok(KcpAccepter(KcpListener::bind(fut.await?, executor)?)) })
    }
}
