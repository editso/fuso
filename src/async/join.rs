use std::{future::Future, pin::Pin, task::Poll};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub struct Join {
    futures: Vec<BoxedFuture<()>>,
}

impl Join {
    pub fn join<F1, F2>(f1: F1, f2: F2) -> Self
    where
        F1: Future<Output = ()> + Send + 'static,
        F2: Future<Output = ()> + Send + 'static,
    {
        Join {
            futures: vec![Box::pin(f1), Box::pin(f2)],
        }
    }

    pub fn add<F>(mut self, fut: F) -> Self
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.futures.push(Box::pin(fut));
        self
    }
}

impl Future for Join {
    type Output = ();

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut futures = Vec::new();
        while let Some(mut future) = self.futures.pop() {
            if let Poll::Pending = Pin::new(&mut future).poll(cx) {
                futures.push(future);
            }
        }

        if futures.is_empty() {
            Poll::Ready(())
        } else {
            drop(std::mem::replace(&mut self.futures, futures));
            Poll::Pending
        }
    }
}
