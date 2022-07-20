mod builder;
pub use builder::*;

use crate::{generator::GeneratorEx, Serve, Socket};
use std::{pin::Pin, sync::Arc};

use crate::{
    generator::Generator, Accepter, AccepterExt, Executor, Factory, FactoryTransfer, Fuso,
    ServerFactory, Stream,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct Server<E, H, SF, CF, SI> {
    pub(crate) bind: Socket,
    pub(crate) executor: E,
    pub(crate) handler: Arc<H>,
    pub(crate) factory: ServerFactory<SF, CF>,
    pub(crate) handshake: Option<Arc<FactoryTransfer<SI>>>,
}

impl<E, H, A, G, SF, CF, SI> Server<E, H, SF, CF, SI>
where
    E: Executor + Send + Clone + 'static,
    A: Accepter<Stream = SI> + Unpin + Send + 'static,
    H: Factory<(ServerFactory<SF, CF>, SI), Output = BoxedFuture<G>> + Send + Sync + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    SF: Factory<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Socket, Output = BoxedFuture<SI>> + Send + Sync + 'static,
    SI: Stream + Send + 'static,
{
    pub async fn run(self) -> crate::Result<()> {
        let mut accepter = self.factory.bind(self.bind.clone()).await?;

        loop {
            let stream = accepter.accept().await?;

            let executor = self.executor.clone();
            let handshake = self.handshake.clone();
            let factory = self.factory.clone();
            let handler = self.handler.clone();

            self.executor.spawn(async move {
                let stream = match handshake.as_ref() {
                    None => Ok(stream),
                    Some(factory) => {
                        log::debug!("start shaking hands");
                        factory.call(stream).await
                    }
                };

                let generator = match stream {
                    Err(e) => {
                        log::warn!("handshake failed {}", e);
                        Err(e)
                    }
                    Ok(stream) => {
                        log::debug!("start processing the connection");
                        handler.call((factory.clone(), stream)).await
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
    H: Factory<(ServerFactory<SF, CF>, SI), Output = BoxedFuture<G>> + Send + Sync + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    SF: Factory<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Socket, Output = BoxedFuture<SI>> + Send + Sync + 'static,
    SI: Stream + Send + 'static,
{
    pub fn bind<T: Into<Socket>>(self, bind: T) -> Self {
        Fuso(Server {
            bind: bind.into(),
            factory: self.0.factory,
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
