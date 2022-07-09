use std::sync::Arc;

use crate::{
    generator::Generator, ClientFactory, Executor, Factory, FactoryTransfer, Fuso, Socket, Stream,
};

use super::{BoxedFuture, Client};

pub struct ClientBuilder<E, CF, S> {
    pub(crate) executor: E,
    pub(crate) handshake: Option<FactoryTransfer<S>>,
    pub(crate) client_factory: ClientFactory<CF>,
}

impl<E, CF, S> ClientBuilder<E, CF, S>
where
    E: Executor + 'static,
    CF: Factory<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    pub fn build<A: Into<Socket>, H, G>(self, socket: A, handler: H) -> Fuso<Client<E, H, CF, S>>
    where
        H: Factory<(ClientFactory<CF>, S), Output = BoxedFuture<G>> + Send + Sync + 'static,
        G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    {
        Fuso(Client {
            socket: socket.into(),
            handler: Arc::new(handler),
            executor: Arc::new(self.executor),
            handshake: self.handshake,
            client_factory: self.client_factory,
        })
    }
}
