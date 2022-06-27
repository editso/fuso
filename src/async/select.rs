use std::{future::Future, pin::Pin, task::Poll};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub struct Select<O> {
    futures: Vec<BoxedFuture<O>>,
}

impl<O> Select<O>
where
    O: Send + 'static,
{
    pub fn select<F1, F2>(f1: F1, f2: F2) -> Self
    where
        F1: Future<Output = O> + Send + 'static,
        F2: Future<Output = O> + Send + 'static,
    {
        Select {
            futures: vec![Box::pin(f1), Box::pin(f2)],
        }
    }

    pub fn add<F>(mut self, fut: F) -> Self
    where
        F: Future<Output = O> + Send + 'static,
    {
        self.futures.push(Box::pin(fut));
        self
    }
}

impl<O> Future for Select<O>
where
    O: Send + 'static,
{
    type Output = O;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        for future in self.futures.iter_mut() {
            match Pin::new(future).poll(cx) {
                Poll::Ready(ready) => return Poll::Ready(ready),
                Poll::Pending => continue,
            }
        }

        Poll::Pending
    }
}
