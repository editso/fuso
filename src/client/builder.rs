use std::sync::Arc;

use crate::{
    generator::Generator, ClientProvider, Executor, Fuso, Provider, ProviderTransfer, Socket,
    Stream,
};

use super::{BoxedFuture, Client};

pub struct ClientBuilder<E, CF, S> {
    pub(crate) executor: E,
    pub(crate) handshake: Option<ProviderTransfer<S>>,
    pub(crate) client_provider: ClientProvider<CF>,
}

impl<E, CF, S> ClientBuilder<E, CF, S>
where
    E: Executor + 'static,
    CF: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    pub fn build<A: Into<Socket>, H, G>(self, socket: A, handler: H) -> Fuso<Client<E, H, CF, S>>
    where
        H: Provider<(ClientProvider<CF>, S), Output = BoxedFuture<G>> + Send + Sync + 'static,
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
