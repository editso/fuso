use std::{ops::Deref, pin::Pin, sync::Arc};

use crate::{ClientProvider, DecorateProvider, Provider, Socket};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct Processor<P, S, O> {
    provider: Arc<P>,
    observer: Option<Arc<O>>,
    decorator: Option<DecorateProvider<S>>,
}

impl<P, S, O> Processor<P, S, O> {
    pub fn new(
        provider: Arc<P>,
        observer: Option<Arc<O>>,
        decorator: Option<DecorateProvider<S>>,
    ) -> Self {
        Self {
            provider,
            observer,
            decorator,
        }
    }

    pub fn observer(&self) -> &Option<Arc<O>> {
        &self.observer
    }

    pub async fn decorate(&self, client: S) -> crate::Result<S> {
        match self.decorator.as_ref() {
            None => Ok(client),
            Some(decorator) => decorator.call(client).await,
        }
    }
}

impl<P, S> Deref for Processor<ClientProvider<P>, S, ()> {
    type Target = Arc<ClientProvider<P>>;

    fn deref(&self) -> &Self::Target {
        &self.provider
    }
}

impl<P, A, S, O> Processor<P, S, O>
where
    P: Provider<Socket, Output = BoxedFuture<A>>,
{
    pub fn bind(&self, socket: Socket) -> BoxedFuture<A> {
        self.provider.call(socket)
    }
}

impl<P, S, O> Clone for Processor<P, S, O> {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
            observer: self.observer.clone(),
            decorator: self.decorator.clone(),
        }
    }
}
