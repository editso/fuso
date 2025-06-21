use crate::{core::BoxedFuture, error};

#[cfg(feature = "fuso-rt-tokio")]
pub mod tokio;

pub trait Runtime {
    fn spawn<F, O>(fut: F) -> ()
    where
        F: std::future::Future<Output = O> + Send + 'static,
        O: Send + 'static;

    fn wait_for<'a, F, O>(
        timeout: std::time::Duration,
        fut: F,
    ) -> BoxedFuture<'a, error::Result<O>>
    where
        F: std::future::Future<Output = O> + Send + 'a;

    fn sleep(timeout: std::time::Duration) -> BoxedFuture<'static, ()>;
}

#[cfg(feature = "fuso-rt-tokio")]
pub fn block_on<T, F>(f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    tokio::block_on(f)
}
