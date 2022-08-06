use std::future::Future;
use std::pin::Pin;

use std::task::{Context, Poll};

pub struct Generate<'a, G>(&'a mut G);

pub trait Generator {
    type Output;

    fn poll_generate(self: Pin<&mut Self>, cx: &mut Context) -> Poll<crate::Result<Self::Output>>;
}

pub trait GeneratorEx: Generator {
    fn next<'a>(&'a mut self) -> Generate<'a, Self>
    where
        Self: Sized,
    {
        Generate(self)
    }
}

impl<'a, G> Future for Generate<'a, G>
where
    G: Generator + Unpin,
{
    type Output = crate::Result<G::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Self::Output> {
        Pin::new(&mut *self.0).poll_generate(cx)
    }
}

impl<T> GeneratorEx for T where T: Generator + Unpin {}
