mod builder;

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

pub use builder::*;

use crate::{
    generator::{Generator, GeneratorEx},
    time, ClientProvider, Executor, Fuso, Provider, DecorateProvider, WrappedProvider, Serve,
    Stream, Address, Socket,
};

pub type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

pub enum Route<S> {
    Forward(S),
    Provider(WrappedProvider<S, ()>),
}

pub struct Client<E, H, CF, S> {
    pub(crate) socket: Socket,
    pub(crate) executor: Arc<E>,
    pub(crate) handler: Arc<H>,
    pub(crate) handshake: Option<DecorateProvider<S>>,
    pub(crate) client_provider: ClientProvider<CF>,
}

impl<E, H, CF, S, G> Client<E, H, CF, S>
where
    E: Executor + 'static,
    H: Provider<(ClientProvider<CF>, S), Output = BoxedFuture<G>> + Send + Sync + 'static,
    CF: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
{
    async fn run(self) -> crate::Result<()> {
        let executor = self.executor;
        let provider = self.client_provider.clone();
        let handshake = self.handshake;

        loop {
            let socket = self.socket.clone();

            let stream = match self.client_provider.connect(socket).await {
                Ok(stream) => {
                    log::info!("connection established");
                    stream
                }
                Err(e) => {
                    log::warn!("connect to {} failed err: {}", self.socket, e);
                    time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            let stream = match handshake.as_ref() {
                Some(handshake) => handshake.call(stream).await,
                None => Ok(stream),
            };

            let stream = match stream {
                Ok(stream) => stream,
                Err(e) => {
                    log::error!("handshake failed {}", e);
                    break Ok(());
                }
            };

            let mut generate = match self.handler.call((provider.clone(), stream)).await {
                Ok(generate) => generate,
                Err(e) => {
                    log::warn!("{}", e);
                    continue;
                }
            };

            loop {
                match generate.next().await {
                    Ok(None) => break,
                    Ok(Some(fut)) => {
                        executor.spawn(fut);
                    }
                    Err(e) => {
                        log::error!("{}", e);
                        break;
                    }
                }
            }
        }
    }
}

impl<E, H, CF, S, G> Fuso<Client<E, H, CF, S>>
where
    E: Executor + 'static,
    H: Provider<(ClientProvider<CF>, S), Output = BoxedFuture<G>> + Send + Sync + 'static,
    CF: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
{
    pub fn run(self) -> Fuso<Serve> {
        Fuso(Serve {
            fut: Box::pin(self.0.run()),
        })
    }
}
