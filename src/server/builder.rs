use std::sync::Arc;

use async_mutex::Mutex;

use crate::{
    encryption::Encryption, handler::Handler, listener::Accepter, Addr, AsyncRead, AsyncWrite,
    Executor,
};

use super::server::Server;

pub type BoxedService<S, E> =
    Box<dyn Handler<Stream = S, Executor = E> + Send + Sync + Unpin + 'static>;
pub type BoxedAccepter<S> = Box<dyn Accepter<Stream = S> + Send + Unpin + 'static>;

pub struct FusoServer<S, E> {
    pub(crate) executor: E,
    pub(crate) services: Vec<BoxedService<S, E>>,
}

impl<S, E> FusoServer<S, E>
where
    E: Executor  + Clone + Send + 'static,
    S: AsyncWrite + AsyncRead + Unpin + Send + 'static,
{
    pub fn encryption(self) -> Encryption<Self> {
        Encryption {
            factory: self,
            ciphers: Default::default(),
        }
    }

    pub fn service<H>(mut self, service: H) -> Self
    where
        H: Handler<Stream = S, Executor = E> + Unpin + Send + Sync + 'static,
    {
        self.services.push(Box::new(service));
        self
    }

    pub fn serve<A>(self, accepter: A) -> Server<S, E>
    where
        A: Accepter<Stream = S> + Unpin + Send + 'static,
    {
        
        Server::new(accepter, self)
    }
}

impl<S, E> Encryption<FusoServer<S, E>>
where
    E: Executor + Clone + Send + 'static,
    S: AsyncWrite + AsyncRead + Unpin + Send + 'static,
{
    pub fn service<H>(mut self, service: H) -> Self
    where
        H: Handler<Stream = S, Executor = E> + Unpin + Send + Sync + 'static,
    {
        self.factory.services.push(Box::new(service));
        self
    }

    pub fn serve<A>(self, accepter: A) -> Server<S, E>
    where
        A: Accepter<Stream = S> + Unpin + Send + 'static,
    {
        Server::new(accepter, self.factory)
    }
}
