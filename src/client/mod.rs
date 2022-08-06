mod builder;

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

pub use builder::*;

use crate::{
    generator::{Generator, GeneratorEx},
    time, ClientProvider, DecorateProvider, Executor, Fuso, Kind, Processor, Provider, Serve,
    Socket, Stream, WrappedProvider,
};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = crate::Result<T>> + Send + 'static>>;

pub enum Route<S> {
    Forward(S),
    Provider(WrappedProvider<S, ()>),
}

pub struct Client<E, H, P, S> {
    pub(crate) socket: Socket,
    pub(crate) executor: Arc<E>,
    pub(crate) handler: Arc<H>,
    pub(crate) maximum_retries: Option<usize>,
    pub(crate) retry_delay: Duration,
    pub(crate) handshake: Option<WrappedProvider<S, (S, Option<DecorateProvider<S>>)>>,
    pub(crate) client_provider: ClientProvider<P>,
}

impl<E, H, P, S, G> Client<E, H, P, S>
where
    E: Executor + 'static,
    S: Stream + Send + 'static,
    P: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    H: Provider<(S, Processor<ClientProvider<P>, S, ()>), Output = BoxedFuture<G>>
        + Send
        + Sync
        + 'static,
{
    async fn run(self) -> crate::Result<()> {
        let executor = self.executor;
        let provider = self.client_provider.clone();
        let handshake = self.handshake;
        let mut retries_count = 0;
        let maximum_retries = self.maximum_retries;

        loop {
            let socket = self.socket.clone();

            let stream = match self.client_provider.connect(socket).await {
                Ok(stream) => {
                    log::info!("connection established");
                    stream
                }
                Err(e) => {
                    log::warn!("connect to {} failed err: {}", self.socket, e);
                    time::sleep(self.retry_delay).await;

                    if let Some(retries) = maximum_retries {
                        if retries > retries_count {
                            break Err(Kind::MaxRetries(retries).into());
                        }
                    }

                    retries_count += 1;

                    log::debug!("reconnect({}) to {} ", retries_count, self.socket);

                    continue;
                }
            };

            let stream = match handshake.as_ref() {
                Some(handshake) => handshake.call(stream).await,
                None => Ok((stream, None)),
            };

            let (server, decorator) = match stream {
                Ok(stream) => stream,
                Err(e) => {
                    log::error!("handshake failed {}", e);
                    break Ok(());
                }
            };

            let processor = Processor::new(Arc::new(provider.clone()), None, decorator);

            let mut generate = match self.handler.call((server, processor)).await {
                Ok(generate) => generate,
                Err(e) => {
                    log::warn!("processing failed ! err: {}", e);
                    time::sleep(self.retry_delay).await;
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
                        log::error!("encountered an error err: {}", e);
                        time::sleep(self.retry_delay).await;
                        break;
                    }
                }
            }
        }
    }
}

impl<E, H, P, S, G> Fuso<Client<E, H, P, S>>
where
    E: Executor + 'static,
    H: Provider<(S, Processor<ClientProvider<P>, S, ()>), Output = BoxedFuture<G>>
        + Send
        + Sync
        + 'static,
    P: Provider<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
{
    pub fn run(self) -> Fuso<Serve> {
        Fuso(Serve {
            fut: Box::pin(self.0.run()),
        })
    }
}
