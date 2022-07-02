use crate::{generator::GeneratorEx, Serve};
use std::{pin::Pin, sync::Arc};

use crate::{
    core::listener::ext::AccepterExt,
    generator::Generator,
    listener::Accepter,
    middleware::FactoryTransfer,
    service::{Factory, ServerFactory},
    Addr, Executor, Fuso, Stream,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct Server<E, H, SF, CF, SI> {
    pub(crate) bind: Addr,
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
    SF: Factory<Addr, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Addr, Output = BoxedFuture<SI>> + Send + Sync + 'static,
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
                    Some(factory) => factory.call(stream).await,
                };

                let generator = match stream {
                    Err(e) => Err(e),
                    Ok(stream) => handler.call((factory.clone(), stream)).await,
                };

                if generator.is_err() {
                    log::warn!("failed to handler {}", stringify!(H));
                    return;
                }

                let mut generator = unsafe { generator.unwrap_unchecked() };

                loop {
                    match generator.next().await {
                        Ok(None) => break,
                        Err(e) => {
                            log::warn!("stop processing");
                            break;
                        }
                        Ok(Some(fut)) => {
                            executor.spawn(fut);
                        }
                    }
                }
            })
        }
    }
}

impl<E, H, A, G, SF, CF, SI> Fuso<Server<E, H, SF, CF, SI>>
where
    E: Executor + Send + Clone + 'static,
    A: Accepter<Stream = SI> + Unpin + Send + 'static,
    H: Factory<(ServerFactory<SF, CF>, SI), Output = BoxedFuture<G>> + Send + Sync + 'static,
    G: Generator<Output = Option<BoxedFuture<()>>> + Unpin + Send + 'static,
    SF: Factory<Addr, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Addr, Output = BoxedFuture<SI>> + Send + Sync + 'static,
    SI: Stream + Send + 'static,
{
    pub fn bind<T: Into<Addr>>(self, bind: T) -> Self {
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
