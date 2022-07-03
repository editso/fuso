use crate::{guard::Timer, service::Transfer, FusoStream};

use std::{pin::Pin, sync::Arc, time::Duration};

use crate::service::Factory;

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct FactoryWrapper<A, O> {
    factory: Arc<Box<dyn Factory<A, Output = BoxedFuture<O>> + Send + Sync + 'static>>,
}

pub struct FactoryTransfer<A> {
    factory: Arc<Box<dyn Factory<A, Output = BoxedFuture<A>> + Send + Sync + 'static>>,
}

pub struct FactoryChain<A> {
    factory: FactoryTransfer<A>,
    next_factory: FactoryTransfer<A>,
}

#[derive(Default)]
pub struct Handshake;

impl Factory<FusoStream> for Handshake {
    type Output = BoxedFuture<FusoStream>;

    fn call(&self, stream: FusoStream) -> Self::Output {
        Box::pin(async move {
            log::debug!("handshake");
            Ok(stream)
        })
    }
}

impl<A, O> FactoryWrapper<A, O> {
    pub fn wrap<F>(factory: F) -> Self
    where
        F: Factory<A, Output = BoxedFuture<O>> + Send + Sync + 'static,
    {
        Self {
            factory: Arc::new(Box::new(factory)),
        }
    }
}

impl<A> FactoryTransfer<A> {
    pub fn wrap<F>(factory: F) -> Self
    where
        F: Factory<A, Output = BoxedFuture<A>> + Send + Sync + 'static,
    {
        Self {
            factory: Arc::new(Box::new(factory)),
        }
    }
}

impl<A> FactoryChain<A> {
    pub fn chain<F1, F2>(factory: F1, next: F2) -> Self
    where
        F1: Factory<A, Output = BoxedFuture<A>> + Send + Sync + 'static,
        F2: Factory<A, Output = BoxedFuture<A>> + Send + Sync + 'static,
    {
        Self {
            factory: FactoryTransfer::wrap(factory),
            next_factory: FactoryTransfer::wrap(next),
        }
    }
}

impl<A, O> Factory<A> for FactoryWrapper<A, O>
where
    A: Send + 'static,
    O: Send + 'static,
{
    type Output = BoxedFuture<O>;

    fn call(&self, cfg: A) -> Self::Output {
        self.factory.call(cfg)
    }
}

impl<A> Factory<A> for FactoryTransfer<A> {
    type Output = BoxedFuture<A>;

    fn call(&self, cfg: A) -> Self::Output {
        self.factory.call(cfg)
    }
}

impl<A> Factory<A> for FactoryChain<A>
where
    A: Send + 'static,
{
    type Output = BoxedFuture<A>;

    fn call(&self, cfg: A) -> Self::Output {
        let this = self.clone();
        Box::pin(async move { this.next_factory.call(this.factory.call(cfg).await?).await })
    }
}

impl<A, O> Clone for FactoryWrapper<A, O> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            factory: self.factory.clone(),
        }
    }
}

impl<A> Clone for FactoryTransfer<A> {
    fn clone(&self) -> Self {
        Self {
            factory: self.factory.clone(),
        }
    }
}

impl<A> Clone for FactoryChain<A> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            factory: self.factory.clone(),
            next_factory: self.next_factory.clone(),
        }
    }
}
