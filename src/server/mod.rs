mod builder;
pub use builder::*;

use crate::{generator::GeneratorEx, Serve, Socket};
use std::{pin::Pin, sync::Arc};

use crate::{
    generator::Generator, Accepter, AccepterExt, Executor, Provider, ProviderTransfer, Fuso,
    ServerProvider, Stream,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct Server<E, H, SF, CF, SI> {
    pub(crate) bind: Socket,
    pub(crate) executor: E,
    pub(crate) handler: Arc<H>,
    pub(crate) provider: ServerProvider<SF, CF>,
    pub(crate) handshake: Option<Arc<ProviderTransfer<SI>>>,
}

impl<E, H, A, G, SF, CF, SI> Server<E, H, SF, CF, SI>
where
    E: Executor + Send + Clone + 'static,
    A: Accepter<Stream = SI> + Unpin + Send + 'static,
    H: Provider<(ServerProvider<SF, CF>, SI), Output = BoxedFuture<G>> + Send + Sync + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    SF: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Provider<Socket, Output = BoxedFuture<SI>> + Send + Sync + 'static,
    SI: Stream + Send + 'static,
{
    pub async fn run(self) -> crate::Result<()> {
        let mut accepter = self.provider.bind(self.bind.clone()).await?;

        log::info!("the server listens on {}", accepter.local_addr()?);

        loop {
            let stream = accepter.accept().await?;

            if let Ok(addr) = stream.peer_addr() {
                log::debug!("connection from {}", addr);
            }

            let executor = self.executor.clone();
            let handshake = self.handshake.clone();
            let provider = self.provider.clone();
            let handler = self.handler.clone();

            self.executor.spawn(async move {
                let stream = match handshake.as_ref() {
                    None => Ok(stream),
                    Some(provider) => {
                        log::debug!("start shaking hands");
                        provider.call(stream).await
                    }
                };

                let generator = match stream {
                    Err(e) => {
                        log::warn!("handshake failed {}", e);
                        Err(e)
                    }
                    Ok(stream) => {
                        log::debug!("start processing the connection");
                        handler.call((provider.clone(), stream)).await
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

impl<E, H, A, G, SF, CF, SI> Fuso<Server<E, H, SF, CF, SI>>
where
    E: Executor + Send + Clone + 'static,
    A: Accepter<Stream = SI> + Unpin + Send + 'static,
    H: Provider<(ServerProvider<SF, CF>, SI), Output = BoxedFuture<G>> + Send + Sync + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    SF: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Provider<Socket, Output = BoxedFuture<SI>> + Send + Sync + 'static,
    SI: Stream + Send + 'static,
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
