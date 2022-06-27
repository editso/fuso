use std::{pin::Pin, sync::Arc};

use crate::{
    listener::Accepter,
    middleware::{FactoryChain, FactoryTransfer},
    service::{Factory, ServerFactory},
    Addr, Executor, Fuso, Stream,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

#[derive(Clone)]
pub struct BoxedEncryption();

#[derive(Default)]
pub struct Builder<S> {
    pub(crate) encryption: Vec<BoxedEncryption>,
    pub(crate) middleware: Option<FactoryTransfer<S>>,
}

impl<S> Builder<S>
where
    S: Stream + Send + 'static,
{
    pub fn add_encryption<T>(mut self, encryption: T) -> Self
    where
        T: Into<BoxedEncryption>,
    {
        self.encryption.push(encryption.into());
        self
    }

    pub fn with_middleware<F>(mut self, middleware: F) -> Self
    where
        F: Factory<S, Output = BoxedFuture<S>> + Send + Sync + 'static,
    {
        match self.middleware.take() {
            None => {
                self.middleware = Some(FactoryTransfer::wrap(middleware));
            }
            Some(wrapper) => {
                self.middleware = Some(FactoryTransfer::wrap(FactoryChain::chain(
                    wrapper, middleware,
                )));
            }
        }
        self
    }

    pub fn build<E, A, SF, CF>(
        self,
        executor: E,
        factory: ServerFactory<SF, CF>,
    ) -> Fuso<super::Server<E, SF, CF, S>>
    where
        SF: Factory<Addr, Output = BoxedFuture<A>> + Send + 'static,
        CF: Factory<Addr, Output = BoxedFuture<S>> + Send + 'static,
        E: Executor + Send + 'static,
        A: Accepter<Stream = S> + Send + 'static,
    {
        Fuso(super::Server {
            executor,
            factory,
            bind: Addr::from(([0, 0, 0, 0], 0)),
            middleware: self.middleware.map(Arc::new),
            encryption: self.encryption,
        })
    }
}
