mod builder;

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

pub use builder::*;

use crate::{
    factory::{FactoryTransfer, FactoryWrapper},
    generator::{Generator, GeneratorEx},
    service::{ClientFactory, Factory},
    Executor, Fuso, Serve, Socket, Stream,
};

pub type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

pub enum Outcome<S> {
    Stream(S),
    Customize(FactoryWrapper<S, ()>),
}

pub struct Client<E, H, CF, S> {
    pub(crate) socket: Socket,
    pub(crate) executor: Arc<E>,
    pub(crate) handler: Arc<H>,
    pub(crate) handshake: Option<FactoryTransfer<S>>,
    pub(crate) client_factory: ClientFactory<CF>,
}

impl<E, H, CF, S, G> Client<E, H, CF, S>
where
    E: Executor + 'static,
    H: Factory<(ClientFactory<CF>, S), Output = BoxedFuture<G>> + Send + Sync + 'static,
    CF: Factory<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
{
    async fn run(self) -> crate::Result<()> {
        let executor = self.executor;
        let factory = self.client_factory.clone();
        let handshake = self.handshake;

        loop {
            let socket = self.socket.clone();

            let stream = match self.client_factory.connect(socket).await {
                Ok(stream) => stream,
                Err(e) => {
                    log::warn!("{}", e);
                    tokio::time::sleep(Duration::from_secs(10)).await;
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

            let mut generate = match self.handler.call((factory.clone(), stream)).await {
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
                        log::debug!("spawn task");
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
    H: Factory<(ClientFactory<CF>, S), Output = BoxedFuture<G>> + Send + Sync + 'static,
    CF: Factory<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
{
    pub fn run(self) -> Fuso<Serve> {
        Fuso(Serve {
            fut: Box::pin(self.0.run()),
        })
    }
}
