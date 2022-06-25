use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll},
};

use crate::{listener::Accepter, Addr, BoxedFuture, Stream};

pub trait Factory<C> {
    type Output;
    type Future: Future<Output = Result<Self::Output, crate::Error>>;

    fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), crate::Error>>;

    fn call(&self, cfg: C) -> Self::Future;
}

pub struct ServerFactory<S, C> {
    pub accepter_factory: Arc<S>,
    pub connector_factory: Arc<C>,
}

impl<SF, CF, S, O> ServerFactory<SF, CF>
where
    SF: Factory<Addr, Output = S, Future = BoxedFuture<'static, crate::Result<S>>> + 'static,
    CF: Factory<Addr, Output = O, Future = BoxedFuture<'static, crate::Result<O>>> + 'static,
    S: Accepter<Stream = O> + 'static,
    O: Stream + Unpin + 'static,
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
