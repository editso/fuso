use std::{marker::PhantomData, pin::Pin, sync::Arc};

use crate::{
    encryption::Encryption,
    listener::Accepter,
    middleware::{FactoryChain, FactoryTransfer, FactoryWrapper},
    service::{Factory, ServerFactory, Transfer},
    Addr, AsyncRead, AsyncWrite, FusoStream, Executor, Fuso, Stream,
};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

#[derive(Clone)]
pub struct BoxedEncryption();

#[derive(Default)]
pub struct Builder<S> {
    pub(crate) encryption: Vec<BoxedEncryption>,
    pub(crate) middleware: Option<FactoryTransfer<FusoStream>>,
    pub(crate) _marked: PhantomData<S>,
}

impl<S> Builder<S>
where
    S: Transfer<Output = FusoStream>,
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
        F: Factory<FusoStream, Output = BoxedFuture<FusoStream>> + Send + Sync + 'static,
    {
        match self.middleware.take() {
            None => {
                self.middleware = Some(FactoryTransfer::wrap(middleware));
            }
            Some(wrapper) => {
                self.middleware = Some(FactoryTransfer::wrap(FactoryChain::chain(
                    wrapper,
                    middleware,
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
            middleware: self.middleware.map(Arc::new),
            bind: Addr::from(([0, 0, 0, 0], 0)),
            encryption: self.encryption,
            _marked: PhantomData,
        })
    }
}
