use std::sync::Arc;

use crate::{
    generator::Generator, ClientProvider, Executor, Fuso, Provider, DecorateProvider, Socket,
    Stream,
};

use super::{BoxedFuture, Client};

pub struct ClientBuilder<E, P, S> {
    pub(crate) executor: E,
    pub(crate) handshake: Option<DecorateProvider<S>>,
    pub(crate) client_provider: ClientProvider<P>,
}

impl<E, P, S> ClientBuilder<E, P, S>
where
    E: Executor + 'static,
    P: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    pub fn build<A: Into<Socket>, H, G>(self, socket: A, handler: H) -> Fuso<Client<E, H, P, S>>
    where
        H: Provider<(ClientProvider<P>, S), Output = BoxedFuture<G>> + Send + Sync + 'static,
        G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    {
        let socket = socket.into();

        Fuso(Client {
            socket: socket.clone(),
            handler: Arc::new(handler),
            executor: Arc::new(self.executor),
            handshake: self.handshake,
            client_provider: self.client_provider.set_server_socket(socket),
        })
    }
}
