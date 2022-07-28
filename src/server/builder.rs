use std::{pin::Pin, sync::Arc};

use crate::{
    generator::Generator, Fuso, Provider, ProviderTransfer, ProviderWrapper, Socket, Stream,
};

use super::{Handshake, Process, Server};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct ServerBuilder<E, SP, S> {
    pub(crate) executor: E,
    pub(crate) is_mixed: bool,
    pub(crate) handshake: Option<Handshake<S>>,
    pub(crate) server_provider: Arc<SP>,
}

impl<E, SP, S> ServerBuilder<E, SP, S>
where
    S: Stream + Send + 'static,
{
    pub fn using_handshake<F>(mut self, handshake: F) -> Self
    where
        F: Provider<S, Output = BoxedFuture<(S, Option<ProviderTransfer<S>>)>>
            + Send
            + Sync
            + 'static,
    {
        self.handshake = Some(ProviderWrapper::wrap(handshake));
        self
    }

    pub fn build<H, G>(self, handler: H) -> Fuso<Server<E, H, SP, S>>
    where
        H: Provider<Process<SP, S>, Output = BoxedFuture<G>> + Send + Sync + 'static,
        G: Generator<Output = Option<BoxedFuture<()>>> + Send + 'static,
    {
        Fuso(Server {
            handler: Arc::new(handler),
            bind: Socket::tcp(([0, 0, 0, 0], 0)),
            executor: self.executor,
            provider: self.server_provider,
            handshake: self.handshake.map(Arc::new),
        })
    }
}
