use std::{pin::Pin, sync::Arc};

use crate::{
    generator::Generator, Factory, FactoryChain, FactoryTransfer, Fuso, ServerFactory, Socket,
    Stream,
};

use super::Server;

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct ServerBuilder<E, SF, CF, S> {
    pub(crate) executor: E,
    pub(crate) is_mixed: bool,
    pub(crate) handshake: Option<FactoryTransfer<S>>,
    pub(crate) server_factory: ServerFactory<SF, CF>,
}

impl<E, SF, CF, S> ServerBuilder<E, SF, CF, S>
where
    S: Stream + Send + 'static,
{
    pub fn with_handshake<F>(mut self, handshake: F) -> Self
    where
        F: Factory<S, Output = BoxedFuture<S>> + Send + Sync + 'static,
    {
        self.handshake = match self.handshake.take() {
            None => Some(FactoryTransfer::wrap(handshake)),
            Some(wrapper) => Some(FactoryTransfer::wrap(FactoryChain::chain(
                wrapper, handshake,
            ))),
        };
        self
    }

    pub fn build<H, G>(self, handler: H) -> Fuso<Server<E, H, SF, CF, S>>
    where
        H: Factory<(ServerFactory<SF, CF>, S), Output = BoxedFuture<G>> + Send + Sync + 'static,
        G: Generator<Output = Option<BoxedFuture<()>>> + Send + 'static,
    {
        Fuso(Server {
            handler: Arc::new(handler),
            bind: Socket::Tcp(([0, 0, 0, 0], 0).into()),
            executor: self.executor,
            factory: self.server_factory,
            handshake: self.handshake.map(Arc::new),
        })
    }
}
