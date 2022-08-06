use std::{future::Future, marker::PhantomData, pin::Pin, sync::Arc, task::Poll};

use crate::{
    server::ServerBuilder, Accepter, AccepterWrapper, Address, NetSocket, Provider, Socket,
};

macro_rules! mix {
    ($fut: expr, $futures: expr) => {
        match $fut.await {
            Ok(a1) => $futures.push(AccepterWrapper::wrap(a1)),
            Err(e) => {
                if !e.is_not_support() {
                    return Err(e);
                }
            }
        }
    };
}

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

#[derive(Clone)]
pub struct MixListener<F1, F2, S> {
    left: Arc<F1>,
    right: Arc<F2>,
    _marked: PhantomData<S>,
}

pub struct MixAccepter<S>(Vec<AccepterWrapper<S>>);

impl<F1, F2, A1, A2, S> Provider<Socket> for MixListener<F1, F2, S>
where
    F1: Provider<Socket, Output = BoxedFuture<A1>> + Send + Sync + 'static,
    F2: Provider<Socket, Output = BoxedFuture<A2>> + Send + Sync + 'static,
    A1: Accepter<Stream = S> + Unpin + Send + 'static,
    A2: Accepter<Stream = S> + Unpin + Send + 'static,
    S: 'static,
{
    type Output = BoxedFuture<MixAccepter<S>>;

    fn call(&self, socket: Socket) -> Self::Output {
        let f1 = self.left.call(socket.clone());
        let f2 = self.right.call(socket);

        Box::pin(async move {
            let mut futures = Vec::new();

            mix!(f1, futures);
            mix!(f2, futures);

            Ok(MixAccepter(futures))
        })
    }
}

impl<S> NetSocket for MixAccepter<S> {
    fn local_addr(&self) -> crate::Result<Address> {
        let mut addrs = Vec::new();
        for accepter in self.0.iter() {
            match accepter.local_addr()? {
                Address::One(socket) => addrs.push(socket),
                Address::Many(sockets) => addrs.extend(sockets),
            }
        }

        Ok(Address::Many(addrs))
    }

    fn peer_addr(&self) -> crate::Result<Address> {
        let mut addrs = Vec::new();
        for accepter in self.0.iter() {
            match accepter.peer_addr()? {
                Address::One(socket) => addrs.push(socket),
                Address::Many(sockets) => addrs.extend(sockets),
            }
        }

        Ok(Address::Many(addrs))
    }
}

impl<S> Accepter for MixAccepter<S>
where
    S: Unpin,
{
    type Stream = S;

    fn poll_accept(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        for accepter in self.0.iter_mut() {
            match Pin::new(accepter).poll_accept(cx)? {
                Poll::Ready(s) => return Poll::Ready(Ok(s)),
                Poll::Pending => {}
            }
        }

        Poll::Pending
    }
}

impl<E, P, A, S, O> ServerBuilder<E, P, S, O>
where
    P: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
{
    pub fn add_accepter<P1, A1>(self, provider: P1) -> ServerBuilder<E, MixListener<P, P1, S>, S, O>
    where
        P1: Provider<Socket, Output = BoxedFuture<A1>> + Send + Sync + 'static,
        A1: Accepter<Stream = S> + Unpin + Send + 'static,
    {
        ServerBuilder {
            executor: self.executor,
            handshake: self.handshake,
            observer: self.observer,
            is_mixed: true,
            server_provider: Arc::new(MixListener {
                left: self.server_provider,
                right: Arc::new(provider),
                _marked: PhantomData,
            }),
        }
    }
}
