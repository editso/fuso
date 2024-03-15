mod kcp;
mod tcp;

mod port_forward;

pub use kcp::*;
pub use tcp::*;

pub use port_forward::*;

use crate::error;

pub struct TokioRuntime;

pub fn block_on<T, F>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

impl super::Runtime for TokioRuntime {
    fn spawn<F, O>(fut: F) -> ()
    where
        F: std::future::Future<Output = O> + Send + 'static,
        O: Send + 'static,
    {
        tokio::spawn(fut);
    }

    fn wait_for<'a, F, O>(
        timeout: std::time::Duration,
        future: F,
    ) -> crate::core::BoxedFuture<'a, crate::error::Result<O>>
    where
        F: std::future::Future<Output = O> + Send + 'a,
    {
        Box::pin(async move {
            tokio::time::timeout(timeout, future)
                .await
                .map_err(|_| error::FusoError::Timeout)
        })
    }

    fn sleep(timeout: std::time::Duration) -> crate::core::BoxedFuture<'static, ()> {
        Box::pin(async move {
            tokio::time::sleep(timeout).await;
        })
    }
}
