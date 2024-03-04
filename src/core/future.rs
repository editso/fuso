use std::{
    pin::Pin,
    task::{Context, Poll},
};

use std::future::Future;

pub struct StoredFuture<'a, O>(Option<Pin<Box<dyn Future<Output = O> + 'a>>>);

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
