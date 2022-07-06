use std::{future::Future, pin::Pin, task::Poll};

type BoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

pub struct Join {
    futures: Vec<BoxedFuture<()>>,
}

pub struct JoinOutput<O1, O2> {
    result: (Option<O1>, Option<O2>),
    fut1: Option<BoxedFuture<O1>>,
    fut2: Option<BoxedFuture<O2>>,
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

impl<O1, O2> Future for JoinOutput<crate::Result<O1>, crate::Result<O2>>
where
    O1: Unpin + 'static,
    O2: Unpin + 'static,
{
    type Output = crate::Result<(O1, O2)>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        loop {
            match self.fut1.take() {
                None if self.fut2.is_none() => break,
                Some(mut fut) => match Pin::new(&mut fut).poll(cx) {
                    Poll::Pending => {
                        drop(std::mem::replace(&mut self.fut1, Some(fut)));
                        if self.fut2.is_none() {
                            break;
                        }
                    }
                    Poll::Ready(Err(e)) => {
                        // 都出错了还 poll？？？，退出！
                        return Poll::Ready(Err(e));
                    }
                    Poll::Ready(r) => drop(std::mem::replace(&mut self.result.0, Some(r))),
                },
                _ => {}
            }

            match self.fut2.take() {
                None if self.fut1.is_none() => break,
                Some(mut fut) => match Pin::new(&mut fut).poll(cx) {
                    Poll::Pending => {
                        drop(std::mem::replace(&mut self.fut2, Some(fut)));
                        break;
                    }
                    Poll::Ready(Err(e)) => {
                        // 都出错了还 poll？？？，退出！
                        return Poll::Ready(Err(e));
                    }
                    Poll::Ready(r) => {
                        drop(std::mem::replace(&mut self.result.1, Some(r)));
                    }
                },
                _ => {}
            }
        }

        if self.fut1.is_none() && self.fut2.is_none() {
            // unwrap_unchecked真丑，考虑下次更换
            let r1 = unsafe { self.result.0.take().unwrap_unchecked().unwrap_unchecked() };
            let r2 = unsafe { self.result.1.take().unwrap_unchecked().unwrap_unchecked() };
            Poll::Ready(Ok((r1, r2)))
        } else {
            Poll::Pending
        }
    }
}

pub fn join_output<F1, F2, O1, O2>(f1: F1, f2: F2) -> JoinOutput<O1, O2>
where
    F1: Future<Output = O1> + Send + 'static,
    F2: Future<Output = O2> + Send + 'static,
    O1: Unpin + 'static,
    O2: Unpin + 'static,
{
    JoinOutput {
        result: (None, None),
        fut1: Some(Box::pin(f1)),
        fut2: Some(Box::pin(f2)),
    }
}
