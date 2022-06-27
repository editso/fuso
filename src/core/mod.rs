pub mod addr;
pub mod connector;
pub mod encryption;
pub mod guard;
pub mod handler;
pub mod listener;
pub mod middleware;
pub mod packet;
pub mod request;
pub mod service;

use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use std::future::Future;

use crate::server::Server;
use crate::{AsyncRead, AsyncWrite, Stream};

pub use self::addr::*;
use self::listener::Accepter;
use self::service::{Factory, Transfer};

type BoxedFuture<O> = Pin<Box<dyn std::future::Future<Output = crate::Result<O>> + Send + 'static>>;

pub type RefContext = Rc<RefCell<Box<dyn Context + Send>>>;

pub struct FusoStream(Box<dyn Stream + Send + 'static>);

pub struct Fuso<T>(pub(crate) T);

pub struct Serve {
    fut: Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + 'static>>,
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

impl<E, SF, CF, A, S> Fuso<Server<E, SF, CF, S>>
where
    E: Executor + Send + Clone + 'static,
    SF: Factory<Addr, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Addr, Output = BoxedFuture<S>> + Send + Sync + 'static,
    A: Accepter<Stream = S> + Send + Unpin + 'static,
    S: Stream + Send + 'static,
{
    pub fn bind<T: Into<Addr>>(self, bind: T) -> Self {
        Fuso(Server {
            bind: bind.into(),
            factory: self.0.factory,
            executor: self.0.executor,
            encryption: self.0.encryption,
            middleware: self.0.middleware,
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

impl AsyncWrite for FusoStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<crate::Result<usize>> {
        Pin::new(&mut *self.0).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut *self.0).poll_flush(cx)
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<()>> {
        Pin::new(&mut *self.0).poll_close(cx)
    }
}

impl AsyncRead for FusoStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut crate::ReadBuf<'_>,
    ) -> std::task::Poll<crate::Result<usize>> {
        Pin::new(&mut *self.0).poll_read(cx, buf)
    }
}

impl FusoStream {
    pub fn new<T>(t: T) -> Self
    where
        T: Stream + Send + 'static,
    {
        Self(Box::new(t))
    }
}

impl<T> Transfer for T
where
    T: Stream + Send + 'static,
{
    type Output = FusoStream;

    fn transfer(self) -> Self::Output {
        FusoStream::new(self)
    }
}
