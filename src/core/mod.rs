pub mod addr;
pub mod connector;
pub mod encryption;
pub mod guard;
pub mod handler;
pub mod listener;
pub mod packet;
pub mod request;
pub mod service;

use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use std::future::Future;

use crate::server::Server;
use crate::{BoxedFuture, Stream};

pub use self::addr::*;
use self::listener::Accepter;
use self::service::Factory;

pub type RefContext = Rc<RefCell<Box<dyn Context + Send>>>;

pub struct Fuso<T>(pub T);

pub struct Serve {
    fut: BoxedFuture<'static, crate::Result<()>>,
}

pub trait Executor {
    fn spawn<F, O>(&self, fut: F)
    where
        F: Future<Output = O> + Send + 'static,
        O: Send + 'static;
}

pub trait Context {
    fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()>;
}

impl<E> Executor for Arc<E>
where
    E: Executor + Send + ?Sized,
{
    fn spawn<F, O>(&self, fut: F)
    where
        F: Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        (**self).spawn(fut)
    }
}

impl<E, SF, CF, S, O> Fuso<Server<E, SF, CF>>
where
    E: Executor + Clone + 'static,
    SF: Factory<Addr, Output = S, Future = BoxedFuture<'static, crate::Result<S>>> + 'static,
    CF: Factory<Addr, Output = O, Future = BoxedFuture<'static, crate::Result<O>>> + 'static,
    S: Accepter<Stream = O> + Unpin + 'static,
    O: Stream + Send + 'static,
{
    pub fn bind<A: Into<Addr>>(self, bind: A) -> Self {
        Fuso(Server {
            bind: bind.into(),
            factory: self.0.factory,
            executor: self.0.executor,
            encryption: self.0.encryption,
        })
    }

    pub fn run(self) -> Fuso<Serve> {
        Fuso(Serve {
            fut: Box::pin(self.0.run()),
        })
    }
}

impl Future for Fuso<Serve> {
    type Output = crate::Result<()>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        Pin::new(&mut self.0.fut).poll(cx)
    }
}
