use std::{pin::Pin, sync::Arc};

use crate::{Addr, Socket};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub trait Factory<C> {
    type Output;

    fn call(&self, arg: C) -> Self::Output;
}

pub trait Transfer {
    type Output;

    fn transfer(self) -> Self::Output;
}

pub struct ClientFactory<C> {
    pub connect_factory: Arc<C>,
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

impl<C, O> ClientFactory<C>
where
    C: Factory<Socket, Output = BoxedFuture<O>>,
    O: Send + 'static,
{
    pub async fn connect<A: Into<Socket>>(&self, socket: A) -> crate::Result<O> {
        self.connect_factory.call(socket.into()).await
    }

    pub fn call_connect<A: Into<Socket>>(&self, socket: A) -> C::Output {
        self.connect_factory.call(socket.into())
    }
}

impl<C> Clone for ClientFactory<C> {
    fn clone(&self) -> Self {
        Self {
            connect_factory: self.connect_factory.clone(),
        }
    }
}
