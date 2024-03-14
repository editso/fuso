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

pub struct StoredFuture<'a, O>(Option<Pin<Box<dyn Future<Output = O> + 'a>>>);

pub struct Select<'a, O>(pub Vec<BoxedFuture<'a, O>>);

pub struct Poller<'a, O>(pub Vec<BoxedFuture<'a, O>>);

unsafe impl<'a, O> Send for StoredFuture<'a, O> {}
unsafe impl<'a, O> Sync for StoredFuture<'a, O> {}

impl<'a, O> StoredFuture<'a, O> {
    pub fn poll<F, Fut>(&mut self, cx: &mut Context<'_>, f: F) -> Poll<O>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = O> + 'a,
    {
        let mut fut = match self.0.take() {
            Some(fut) => fut,
            None => Box::pin(f()),
        };

        Pin::new(&mut fut).poll(cx)
    }
}

impl<'a, O> StoredFuture<'a, O> {
    pub fn new() -> Self {
        Self(Default::default())
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
                self.0.remove(idx);
                return Poll::Ready(o);
            }
        }

        Poll::Pending
    }
}
