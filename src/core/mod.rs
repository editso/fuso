pub mod addr;
pub mod connector;
pub mod encryption;
pub mod guard;
pub mod handler;
pub mod listener;
pub mod packet;
pub mod request;
pub mod service;

use std::rc::Rc;
use std::sync::Arc;
use std::{cell::RefCell, net::SocketAddr};

use std::future::Future;

pub use self::addr::*;

pub type RefContext = Rc<RefCell<Box<dyn Context + Send>>>;

pub trait Executor{

    fn spawn<F, O>(&self, fut: F)
    where
        F: Future<Output = O> + Send + 'static,
        O: Send + 'static;
}

pub trait Server {
    fn local_addr(&self) -> SocketAddr;
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
        O: Send + 'static
    {
        (**self).spawn(fut)
    }
}
