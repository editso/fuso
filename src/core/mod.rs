mod factory;
pub use factory::*;

mod accepter;
pub use accepter::*;

pub mod addr;
pub use addr::*;

pub mod encryption;
pub mod generator;
pub mod guard;
pub mod listener;
pub mod protocol;

use std::cell::RefCell;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use std::future::Future;

use crate::{AsyncRead, AsyncWrite, Stream};



pub type RefContext = Rc<RefCell<Box<dyn Context + Send>>>;

pub struct FusoStream(Box<dyn Stream + Send + 'static>);

pub struct Fuso<T>(pub(crate) T);

pub struct Serve {
    pub(crate) fut: Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + 'static>>,
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

unsafe impl Sync for FusoStream {}

impl<T> factory::Transfer for T
where
    T: Stream + Send + 'static,
{
    type Output = FusoStream;

    fn transfer(self) -> Self::Output {
        FusoStream::new(self)
    }
}
