use std::{pin::Pin, sync::Arc};

use crate::{
    generator::Generator, DecorateProvider, Environ, Fuso, Provider, Socket, Stream,
    WrappedProvider,
};

use super::{Handshake, Processor, Server};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct ServerBuilder<E, P, S, O> {
    pub(crate) executor: E,
    pub(crate) is_mixed: bool,
    pub(crate) webhook: Option<Arc<O>>,
    pub(crate) handshake: Option<Handshake<S>>,
    pub(crate) server_provider: Arc<P>,
}

impl<E, P, S, O> ServerBuilder<E, P, S, O>
where
    S: Stream + Send + 'static,
{
    pub fn using_handshake<F>(mut self, handshake: F) -> Self
    where
        F: Provider<S, Output = BoxedFuture<(S, Option<DecorateProvider<S>>)>>
            + Send
            + Sync
            + 'static,
    {
        self.handshake = Some(WrappedProvider::wrap(handshake));
        self
    }

    pub fn build<H, G>(self, handler: H) -> Fuso<Server<E, H, P, S, O>>
    where
        G: Generator<Output = Option<BoxedFuture<()>>> + Send + 'static,
        H: Provider<
                (S, Processor<P, S, O>),
                Output = BoxedFuture<(G, Arc<dyn Environ + Send + Sync + 'static>)>,
            > + Send
            + Sync
            + 'static,
    {
        Fuso(Server {
            handler: Arc::new(handler),
            bind: Socket::tcp(([0, 0, 0, 0], 0)),
            executor: self.executor,
            provider: self.server_provider,
            observer: self.webhook,
            handshake: self.handshake.map(Arc::new),
            controller: None,
        })
    }
}
