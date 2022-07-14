use std::{future::Future, marker::PhantomData, pin::Pin, sync::Arc, task::Poll};

use crate::{server::ServerBuilder, Accepter, AccepterWrapper, Factory, ServerFactory, Socket};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

#[derive(Clone)]
pub struct MixListener<F1, F2, S> {
    left: Arc<F1>,
    right: Arc<F2>,
    _marked: PhantomData<S>,
}

pub struct MixAccepter<S>(Vec<AccepterWrapper<S>>);

impl<F1, F2, A1, A2, S> Factory<Socket> for MixListener<F1, F2, S>
where
    F1: Factory<Socket, Output = BoxedFuture<A1>> + Send + Sync + 'static,
    F2: Factory<Socket, Output = BoxedFuture<A2>> + Send + Sync + 'static,
    A1: Accepter<Stream = S> + Unpin + Send + 'static,
    A2: Accepter<Stream = S> + Unpin + Send + 'static,
    S: 'static,
{
    type Output = BoxedFuture<MixAccepter<S>>;

    fn call(&self, socket: Socket) -> Self::Output {
        let socket = socket.if_stream_mixed(true);

        let f1 = self.left.call(socket.clone());
        let f2 = self.right.call(socket);

        Box::pin(async move {
            Ok(MixAccepter(vec![
                AccepterWrapper::wrap(f1.await?),
                AccepterWrapper::wrap(f2.await?),
            ]))
        })
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
        log::debug!("poll mixed listening");

        for accepter in self.0.iter_mut() {
            match Pin::new(accepter).poll_accept(cx)? {
                Poll::Ready(s) => return Poll::Ready(Ok(s)),
                Poll::Pending => {}
            }
        }

        Poll::Pending
    }
}

impl<E, SF, CF, S, A1> ServerBuilder<E, SF, CF, S>
where
    SF: Factory<Socket, Output = BoxedFuture<A1>> + Send + Sync + 'static,
    A1: Accepter<Stream = S> + Unpin + Send + 'static,
{
    pub fn add_accepter<SF2, A2>(
        self,
        factory: SF2,
    ) -> ServerBuilder<E, MixListener<SF, SF2, S>, CF, S>
    where
        SF2: Factory<Socket, Output = BoxedFuture<A2>> + Send + Sync + 'static,
        A2: Accepter<Stream = S> + Unpin + Send + 'static,
    {
        ServerBuilder {
            executor: self.executor,
            handshake: self.handshake,
            is_mixed: true,
            server_factory: ServerFactory {
                connector_factory: self.server_factory.connector_factory,
                accepter_factory: Arc::new(MixListener {
                    left: self.server_factory.accepter_factory,
                    right: Arc::new(factory),
                    _marked: PhantomData,
                }),
            },
        }
    }
}
