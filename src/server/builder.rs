use std::{pin::Pin, sync::Arc};

use crate::{
    generator::Generator,
    factory::{FactoryChain, FactoryTransfer},
    service::{Factory, ServerFactory},
    Fuso, Stream,
};

use super::Server;

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct ServerBuilder<E, SF, CF, S> {
    pub(crate) executor: E,
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
        match self.handshake.take() {
            None => {
                self.handshake = Some(FactoryTransfer::wrap(handshake));
            }
            Some(wrapper) => {
                self.handshake = Some(FactoryTransfer::wrap(FactoryChain::chain(
                    wrapper, handshake,
                )));
            }
        }
        self
    }

    pub fn build<H, G>(self, handler: H) -> Fuso<Server<E, H, SF, CF, S>>
    where
        H: Factory<(ServerFactory<SF, CF>, S), Output = BoxedFuture<G>> + Send + Sync + 'static,
        G: Generator<Output = Option<BoxedFuture<()>>> + Send + 'static,
    {
        Fuso(Server {
            handler: Arc::new(handler),
            bind: ([0, 0, 0, 0], 0).into(),
            executor: self.executor,
            factory: self.server_factory,
            handshake: self.handshake.map(Arc::new),
        })
    }
}
