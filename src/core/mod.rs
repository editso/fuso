mod provider;
pub use provider::*;

pub mod compress;

mod accepter;
pub use accepter::*;

mod boxed;
pub use boxed::*;

mod socket;
pub use socket::*;

pub mod encryption;
pub mod generator;
pub mod guard;
pub mod mixing;
pub mod protocol;

use std::sync::Arc;
use std::{fmt::Display, pin::Pin};

use std::future::Future;

pub struct Fuso<T>(pub(crate) T);

pub struct Serve {
    pub(crate) fut: Pin<Box<dyn std::future::Future<Output = crate::Result<()>> + 'static>>,
}

pub enum Task<T> {
    Tokio(tokio::task::JoinHandle<T>),
}

pub trait ResultDisplay {
    fn display(&self) -> String;
}

pub trait Executor {
    fn spawn<F, O>(&self, fut: F) -> Task<O>
    where
        F: Future<Output = O> + Send + 'static,
        O: Send + 'static;
}

impl<E> Executor for Arc<E>
where
    E: Executor + Send + ?Sized,
{
    fn spawn<F, O>(&self, fut: F) -> Task<O>
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

impl<O> Task<O> {
    pub fn abort(&self) {
        match self {
            Task::Tokio(tokio) => {
                tokio.abort();
            }
        }
    }
}

impl<T, E> ResultDisplay for std::result::Result<T, E>
where
    T: Display,
    E: Display,
{
    fn display(&self) -> String {
        match self {
            Ok(fmt) => format!("{}", fmt),
            Err(fmt) => format!("err: {}", fmt),
        }
    }
}
