use crate::FusoStream;

use std::{pin::Pin, sync::Arc};

use crate::Socket;

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
    SF: Factory<Socket, Output = BoxedFuture<S>> + 'static,
    CF: Factory<Socket, Output = BoxedFuture<O>> + 'static,
    S: 'static,
    O: 'static,
{
    #[inline]
    pub async fn bind<Sock: Into<Socket>>(&self, socket: Sock) -> crate::Result<S> {
        let socket = socket.into();
        log::debug!("{}", socket);

        self.accepter_factory.call(socket).await
    }

    #[inline]
    pub async fn connect<Sock: Into<Socket>>(&self, socket: Sock) -> crate::Result<O> {
        let socket = socket.into();
        self.connector_factory.call(socket).await
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
}

impl<C, O> Factory<Socket> for ClientFactory<C>
where
    C: Factory<Socket, Output = BoxedFuture<O>>,
    O: Send + 'static,
{
    type Output = C::Output;

    fn call(&self, arg: Socket) -> Self::Output {
        self.connect_factory.call(arg)
    }
}

impl<C> Clone for ClientFactory<C> {
    fn clone(&self) -> Self {
        Self {
            connect_factory: self.connect_factory.clone(),
        }
    }
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
