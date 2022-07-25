use std::{future::Future, pin::Pin, task::Poll};

use crate::{
    mixing::MixListener, server::ServerBuilder, Accepter, Executor, Provider, ProviderWrapper,
    FusoStream, NetSocket, Socket, ToBoxStream, UdpSocket, Address,
};

use super::KcpListener;

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct KcpAccepterProvider<C, E> {
    provider: ProviderWrapper<Socket, C>,
    executor: E,
}

pub struct KcpAccepter<C, E>(KcpListener<C, E>);

impl<C, E> NetSocket for KcpAccepter<C, E>
where
    C: NetSocket,
{
    fn peer_addr(&self) -> crate::Result<crate::Address> {
        self.0.core.peer_addr()
    }

    fn local_addr(&self) -> crate::Result<crate::Address> {
        self.0.core.local_addr()
    }
}

impl NetSocket for std::net::TcpStream{
    fn peer_addr(&self) -> crate::Result<crate::Address> {
        Ok(Address::Single(Socket::tcp(self.peer_addr()?)))
    }

    fn local_addr(&self) -> crate::Result<crate::Address> {
        Ok(Address::Single(Socket::tcp(self.local_addr()?)))
    }
}

impl<C, E> Accepter for KcpAccepter<C, E>
where
    C: UdpSocket + Clone + Sync + Unpin + Send + 'static,
    E: Executor + Clone + Sync + Send + Unpin + 'static,
{
    type Stream = FusoStream;

    fn poll_accept(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<crate::Result<Self::Stream>> {
        Pin::new(&mut self.0)
            .poll_accept(cx)?
            .map(|kcp| Ok(kcp.into_boxed_stream()))
    }
}

impl<E, SF, CF, A1> ServerBuilder<E, SF, CF, FusoStream>
where
    SF: Provider<Socket, Output = BoxedFuture<A1>> + Send + Sync + 'static,
    A1: Accepter<Stream = FusoStream> + Unpin + Send + 'static,
    E: Executor + Clone + Sync + Send + Unpin + 'static,
{
    pub fn with_kcp_accepter<F, U>(
        self,
        provider: F,
        executor: E,
    ) -> ServerBuilder<E, MixListener<SF, KcpAccepterProvider<U, E>, FusoStream>, CF, FusoStream>
    where
        F: Provider<Socket, Output = BoxedFuture<U>> + Send + Sync + 'static,
        U: UdpSocket + Clone + Sync + Unpin + Send + 'static,
    {
        self.add_accepter(KcpAccepterProvider {
            executor,
            provider: ProviderWrapper::wrap(provider),
        })
    }
}

impl<C, E> Provider<Socket> for KcpAccepterProvider<C, E>
where
    C: UdpSocket + Clone + Sync + Unpin + Send + 'static,
    E: Executor + Clone + Sync + Send + 'static,
{
    type Output = BoxedFuture<KcpAccepter<C, E>>;

    fn call(&self, arg: Socket) -> Self::Output {
        let fut = self.provider.call(arg);
        let executor = self.executor.clone();
        Box::pin(async move { Ok(KcpAccepter(KcpListener::bind(fut.await?, executor)?)) })
    }
}
