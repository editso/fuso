mod builder;
pub use builder::*;

use crate::{generator::GeneratorEx, ProviderWrapper, Serve, Socket};
use std::{pin::Pin, sync::Arc};

use crate::{
    generator::Generator, Accepter, AccepterExt, Executor, Fuso, Provider, ProviderTransfer, Stream,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;
pub type Process<SP, S> = (Arc<SP>, S, Option<ProviderTransfer<S>>);
pub type Handshake<S> = ProviderWrapper<S, (S, Option<ProviderTransfer<S>>)>;

pub struct Server<E, H, SP, S> {
    pub(crate) bind: Socket,
    pub(crate) executor: E,
    pub(crate) handler: Arc<H>,
    pub(crate) provider: Arc<SP>,
    pub(crate) handshake: Option<Arc<Handshake<S>>>,
}

impl<E, H, A, G, SP, S> Server<E, H, SP, S>
where
    E: Executor + Send + Clone + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    H: Provider<Process<SP, S>, Output = BoxedFuture<G>> + Send + Sync + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    SP: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    pub async fn run(self) -> crate::Result<()> {
        let mut accepter = self.provider.call(self.bind.clone()).await?;

        log::info!("the server listens on {}", accepter.local_addr()?);

        loop {
            let client = accepter.accept().await?;

            if let Ok(addr) = client.peer_addr() {
                log::debug!("connection from {}", addr);
            }

            let executor = self.executor.clone();
            let handshake = self.handshake.clone();
            let provider = self.provider.clone();
            let handler = self.handler.clone();

            self.executor.spawn(async move {
                let client = match handshake.as_ref() {
                    None => Ok((client, None)),
                    Some(provider) => {
                        log::debug!("start shaking hands");
                        provider.call(client).await
                    }
                };

                let generator = match client {
                    Err(e) => {
                        log::warn!("handshake failed {}", e);
                        Err(e)
                    }
                    Ok((client, transporter)) => {
                        log::debug!("start processing the connection");
                        handler.call((provider, client, transporter)).await
                    }
                };

                if generator.is_err() {
                    log::warn!("Failed to handle connection {}", unsafe {
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
            });
        }
    }
}

impl<E, H, A, G, SP, S> Fuso<Server<E, H, SP, S>>
where
    E: Executor + Send + Clone + 'static,
    A: Accepter<Stream = S> + Unpin + Send + 'static,
    H: Provider<Process<SP, S>, Output = BoxedFuture<G>> + Send + Sync + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    SP: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    S: Stream + Send + 'static,
{
    pub fn bind<T: Into<Socket>>(self, bind: T) -> Self {
        Fuso(Server {
            bind: bind.into(),
            provider: self.0.provider,
            executor: self.0.executor,
            handshake: self.0.handshake,
            handler: self.0.handler,
        })
    }

    pub fn run(self) -> Fuso<Serve> {
        Fuso(Serve {
            fut: Box::pin(self.0.run()),
        })
    }
}
