use std::{pin::Pin, sync::Arc};

use crate::Socket;

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub struct WrappedProvider<A, O> {
    provider: Arc<Box<dyn Provider<A, Output = BoxedFuture<O>> + Send + Sync + 'static>>,
}

pub struct DecorateProvider<T> {
    provider: Arc<Box<dyn Provider<T, Output = BoxedFuture<T>> + Send + Sync + 'static>>,
}

pub struct ProviderChain<A> {
    left: DecorateProvider<A>,
    right: DecorateProvider<A>,
}

pub trait Provider<C> {
    type Output;

    fn call(&self, arg: C) -> Self::Output;
}

pub struct ClientProvider<C> {
    pub server_address: Socket,
    pub connect_provider: Arc<C>,
}

impl<C, O> ClientProvider<C>
where
    C: Provider<Socket, Output = BoxedFuture<O>>,
    O: Send + 'static,
{
    pub(crate) fn set_server_socket(mut self, socket: Socket) -> Self {
        self.server_address = socket;
        self
    }

    pub(crate) fn default_socket(&self) -> &Socket {
        &self.server_address
    }

    pub fn connect<A: Into<Socket>>(&self, socket: A) -> BoxedFuture<O> {
        self.connect_provider.call(socket.into())
    }
}

impl<C, O> Provider<Socket> for ClientProvider<C>
where
    C: Provider<Socket, Output = BoxedFuture<O>>,
    O: Send + 'static,
{
    type Output = C::Output;

    fn call(&self, arg: Socket) -> Self::Output {
        self.connect_provider.call(arg)
    }
}

impl<C> Clone for ClientProvider<C> {
    fn clone(&self) -> Self {
        Self {
            server_address: self.server_address.clone(),
            connect_provider: self.connect_provider.clone(),
        }
    }
}

impl<A, O> WrappedProvider<A, O> {
    pub fn wrap<F>(provider: F) -> Self
    where
        F: Provider<A, Output = BoxedFuture<O>> + Send + Sync + 'static,
    {
        Self {
            provider: Arc::new(Box::new(provider)),
        }
    }
}

impl<A> DecorateProvider<A> {
    pub fn wrap<F>(provider: F) -> Self
    where
        F: Provider<A, Output = BoxedFuture<A>> + Send + Sync + 'static,
    {
        Self {
            provider: Arc::new(Box::new(provider)),
        }
    }
}

impl<A> ProviderChain<A> {
    pub fn chain<F1, F2>(provider: F1, next: F2) -> Self
    where
        F1: Provider<A, Output = BoxedFuture<A>> + Send + Sync + 'static,
        F2: Provider<A, Output = BoxedFuture<A>> + Send + Sync + 'static,
    {
        Self {
            left: DecorateProvider::wrap(provider),
            right: DecorateProvider::wrap(next),
        }
    }
}

impl<A, O> Provider<A> for WrappedProvider<A, O>
where
    A: Send + 'static,
    O: Send + 'static,
{
    type Output = BoxedFuture<O>;

    fn call(&self, cfg: A) -> Self::Output {
        self.provider.call(cfg)
    }
}

impl<A> Provider<A> for DecorateProvider<A> {
    type Output = BoxedFuture<A>;

    fn call(&self, cfg: A) -> Self::Output {
        self.provider.call(cfg)
    }
}

impl<A> Provider<A> for ProviderChain<A>
where
    A: Send + 'static,
{
    type Output = BoxedFuture<A>;

    fn call(&self, cfg: A) -> Self::Output {
        let this = self.clone();
        Box::pin(async move { this.right.call(this.left.call(cfg).await?).await })
    }
}

impl<A, O> Clone for WrappedProvider<A, O> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
        }
    }
}

impl<A> Clone for DecorateProvider<A> {
    fn clone(&self) -> Self {
        Self {
            provider: self.provider.clone(),
        }
    }
}

impl<A> Clone for ProviderChain<A> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            left: self.left.clone(),
            right: self.right.clone(),
        }
    }
}
