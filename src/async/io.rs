use std::{future::Future, pin::Pin, sync::Arc, task::Poll};

use async_mutex::Mutex;

use crate::{
    ext::{AsyncReadExt, AsyncWriteExt},
    AsyncRead, AsyncWrite,
};

type BoxedFuture = Pin<Box<dyn Future<Output = crate::Result<()>> + Send + 'static>>;

pub struct Forward {
    results: Vec<crate::Result<()>>,
    futures: Vec<BoxedFuture>,
}

impl Future for Forward {
    type Output = Vec<crate::Result<()>>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let mut results = std::mem::replace(&mut self.results, Default::default());
        let mut futures = Vec::new();
        while let Some(mut future) = self.futures.pop() {
            match Pin::new(&mut future).poll(cx) {
                Poll::Pending => futures.push(future),
                Poll::Ready(r) => {
                    results.push(r);
                }
            }
        }

        if futures.is_empty() {
            return Poll::Ready(results);
        }

        drop(std::mem::replace(&mut self.futures, futures));
        drop(std::mem::replace(&mut self.results, results));

        Poll::Pending
    }
}

pub fn forward<S1, S2>(s1: S1, s2: S2) -> Forward
where
    S1: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    S2: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let s1 = Arc::new(Mutex::new(s1));
    let s2 = Arc::new(Mutex::new(s2));

    fn copy<S1, S2>(s1: Arc<Mutex<S1>>, s2: Arc<Mutex<S2>>) -> BoxedFuture
    where
        S1: AsyncRead + AsyncWrite + Unpin + Send + 'static,
        S2: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        Box::pin(async move {
            let mut buf = unsafe{
                let mut buf = Vec::with_capacity(1500);
                buf.set_len(1500);
                buf
            };

            log::debug!("start forwarding ...");

            loop {
                let r = s1.lock().await.read(&mut buf).await;

                if r.is_err() {
                    return Err(unsafe { r.unwrap_err_unchecked() });
                }

                let n = r.unwrap();

                if n == 0 {
                    return Ok(());
                }

                let r = s2.lock().await.write_all(&buf[..n]).await;

                if r.is_err() {
                    return Err(unsafe { r.unwrap_err_unchecked() });
                }
            }
        })
    }

    Forward {
        results: Default::default(),
        futures: vec![copy(s1.clone(), s2.clone()), copy(s1, s2)],
    }
}
