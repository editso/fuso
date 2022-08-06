use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use crate::{
    generator::Generator, ClientProvider, DecorateProvider, Executor, Fuso, Processor, Provider,
    Socket, Stream, WrappedProvider,
};

use super::Client;

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct ClientBuilder<E, P, S> {
    pub(crate) executor: E,
    pub(crate) retry_delay: Option<Duration>,
    pub(crate) maximum_retries: Option<usize>,
    pub(crate) handshake: Option<WrappedProvider<S, (S, Option<DecorateProvider<S>>)>>,
    pub(crate) client_provider: ClientProvider<P>,
}

impl<E, P, S> ClientBuilder<E, P, S>
where
    E: Executor + 'static,
    P: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    pub fn using_handshake<H>(mut self, handshake: H) -> Self
    where
        H: Provider<S, Output = BoxedFuture<(S, Option<DecorateProvider<S>>)>>
            + Send
            + Sync
            + 'static,
    {
        self.handshake = Some(WrappedProvider::wrap(handshake));
        self
    }

    pub fn build<A: Into<Socket>, H, G>(self, socket: A, handler: H) -> Fuso<Client<E, H, P, S>>
    where
        G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
        H: Provider<(S, Processor<ClientProvider<P>, S, ()>), Output = BoxedFuture<G>>
            + Send
            + Sync
            + 'static,
    {
        let socket = socket.into();

        Fuso(Client {
            socket: socket.clone(),
            maximum_retries: self.maximum_retries,
            retry_delay: self.retry_delay.unwrap_or(Duration::from_secs(5)),
            handler: Arc::new(handler),
            executor: Arc::new(self.executor),
            handshake: self.handshake,
            client_provider: self.client_provider.set_server_socket(socket),
        })
    }
}
