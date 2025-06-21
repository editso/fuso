use std::{
    pin::Pin,
    task::{Context, Poll},
};

use std::future::Future;

use super::BoxedFuture;

#[macro_export]
macro_rules! select {
    ($($fut: expr),*) => {
        $crate::core::future::Select(vec![
            $(
                {
                    let fut: BoxedFuture<'a, _> = Box::pin($fut);
                    fut
                }
            ),*
        ]).await
    };
}

pub struct LazyFuture<'a, O>(Option<Pin<Box<dyn Future<Output = O> + 'a>>>);

pub struct Select<'a, O>(pub Vec<BoxedFuture<'a, O>>);

pub struct Poller<'a, O>(pub Vec<BoxedFuture<'a, O>>);

unsafe impl<'a, O> Send for LazyFuture<'a, O> {}
unsafe impl<'a, O> Sync for LazyFuture<'a, O> {}

impl<'a, O> LazyFuture<'a, O> {
    pub fn poll<F, Fut>(&mut self, cx: &mut Context<'_>, f: F) -> Poll<O>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = O> + 'a,
    {
        let mut fut = match self.0.take() {
            Some(fut) => fut,
            None => Box::pin(f()),
        };

        match Pin::new(&mut fut).poll(cx) {
            Poll::Ready(o) => Poll::Ready(o),
            Poll::Pending => {
                self.0.replace(fut);
                Poll::Pending
            }
        }
    }
}

impl<'a, O> LazyFuture<'a, O> {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn reset(&mut self) {
        drop(self.0.take());
    }
}

impl<'a, O> Poller<'a, O> {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add<F>(&mut self, fut: F)
    where
        F: Future<Output = O> + Send + 'a,
    {
        self.0.push(Box::pin(fut))
    }

    pub fn has_more(&self) -> bool {
        self.0.len() > 0
    }
}

impl<'a, O> Select<'a, O> {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add<F>(&mut self, fut: F)
    where
        F: std::future::Future<Output = O> + Send + 'a,
    {
        self.0.push(Box::pin(fut))
    }
}

impl<O> std::future::Future for Select<'_, O> {
    type Output = O;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        for fut in self.0.iter_mut() {
            match Pin::new(fut).poll(cx) {
                Poll::Pending => continue,
                Poll::Ready(o) => return Poll::Ready(o),
            }
        }

        Poll::Pending
    }
}

impl<O> std::future::Future for Poller<'_, O> {
    type Output = O;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        for (idx, fut) in self.0.iter_mut().enumerate() {
            if let Poll::Ready(o) = Pin::new(fut).poll(cx) {
                drop(self.0.remove(idx));
                return Poll::Ready(o);
            }
        }

        Poll::Pending
    }
}
