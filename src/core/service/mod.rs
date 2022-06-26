use std::{pin::Pin, sync::Arc};

use crate::Addr;

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub trait Factory<C> {
    type Output;

    fn call(&self, cfg: C) -> Self::Output;
}

pub trait Transfer {
    type Output;

    fn transfer(self) -> Self::Output;
}

pub struct ServerFactory<S, C> {
    pub accepter_factory: Arc<S>,
    pub connector_factory: Arc<C>,
}

impl<SF, CF, S, O> ServerFactory<SF, CF>
where
    SF: Factory<Addr, Output = BoxedFuture<S>> + 'static,
    CF: Factory<Addr, Output = BoxedFuture<O>> + 'static,
    S: 'static,
    O: 'static,
{
    #[inline]
    pub async fn bind<Socket: Into<Addr>>(&self, addr: Socket) -> crate::Result<S> {
        let addr = addr.into();
        log::debug!("{}", addr);

        self.accepter_factory.call(addr).await
    }

    #[inline]
    pub async fn connect<Socket: Into<Addr>>(&self, addr: Socket) -> crate::Result<O> {
        let addr = addr.into();
        log::debug!("{}", addr);
        self.connector_factory.call(addr).await
    }
}

impl<S, C> Clone for ServerFactory<S, C> {
    fn clone(&self) -> Self {
        Self {
            accepter_factory: self.accepter_factory.clone(),
            connector_factory: self.connector_factory.clone(),
        }
    }
}
