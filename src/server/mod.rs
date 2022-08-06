mod builder;

pub use builder::*;

use crate::{
    generator::GeneratorEx, DecorateProvider, Observer, Processor, Serve, Socket, WrappedProvider,
};
use std::{pin::Pin, sync::Arc};

use crate::{generator::Generator, Accepter, AccepterExt, Executor, Fuso, Provider, Stream};

pub type Handshake<S> = WrappedProvider<S, (S, Option<DecorateProvider<S>>)>;

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct Server<E, H, P, S, O> {
    pub(crate) bind: Socket,
    pub(crate) executor: E,
    pub(crate) handler: Arc<H>,
    pub(crate) provider: Arc<P>,
    pub(crate) observer: Option<Arc<O>>,
    pub(crate) handshake: Option<Arc<Handshake<S>>>,
}

impl<E, H, A, G, P, S, O> Server<E, H, P, S, O>
where
    E: Executor + Send + Clone + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    H: Provider<(S, Processor<P, S, O>), Output = BoxedFuture<G>> + Send + Sync + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    S: Stream + Send + 'static,
    O: Observer + Send + Sync + 'static,
    P: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
{
    pub async fn run(self) -> crate::Result<()> {
        let mut accepter = self.provider.call(self.bind.clone()).await?;

        log::info!("the server listens on {}", accepter.local_addr()?);

        loop {
            let client = accepter.accept().await?;

            let executor = self.executor.clone();
            let handshake = self.handshake.clone();
            let provider = self.provider.clone();
            let handler = self.handler.clone();
            let observer = self.observer.clone();

            let client_addr = match client.peer_addr() {
                Ok(addr) => addr,
                Err(e) => {
                    log::error!("{}", e);
                    continue;
                }
            };

            observer.on_connect(&client_addr);

            self.executor.spawn(async move {
                let now = std::time::Instant::now();

                let client = match handshake.as_ref() {
                    None => Ok((client, None)),
                    Some(provider) => {
                        log::debug!("start shaking hands");
                        provider.call(client).await
                    }
                };

                observer.on_handshake(&client_addr);

                let generator = match client {
                    Err(e) => {
                        log::warn!("handshake failed {}", e);
                        Err(e)
                    }
                    Ok((client, decorator)) => {
                        log::debug!("start processing the connection");
                        handler
                            .call((
                                client,
                                Processor::new(provider, observer.clone(), decorator),
                            ))
                            .await
                    }
                };

                if generator.is_err() {
                    log::warn!("failed to handle connection {}", unsafe {
                        generator.unwrap_err_unchecked()
                    });
                    return;
                }

                let mut generator = unsafe { generator.unwrap_unchecked() };

                loop {
                    match generator.next().await {
                        Ok(None) => break,
                        Err(e) => {
                            log::warn!("An error occurred {}", e);
                            break;
                        }
                        Ok(Some(fut)) => {
                            executor.spawn(fut);
                        }
                    }
                }

                log::warn!("stop processing");

                observer.on_stop(now,&client_addr);
            });
        }
    }
}

impl<E, H, A, G, P, S, O> Fuso<Server<E, H, P, S, O>>
where
    E: Executor + Send + Clone + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    H: Provider<(S, Processor<P, S, O>), Output = BoxedFuture<G>> + Send + Sync + 'static,
    O: Observer + Sync + Send + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    P: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    pub fn bind<T: Into<Socket>>(self, bind: T) -> Self {
        Fuso(Server {
            bind: bind.into(),
            provider: self.0.provider,
            executor: self.0.executor,
            handshake: self.0.handshake,
            handler: self.0.handler,
            observer: self.0.observer,
        })
    }

    pub fn run(self) -> Fuso<Serve> {
        Fuso(Serve {
            fut: Box::pin(self.0.run()),
        })
    }
}
