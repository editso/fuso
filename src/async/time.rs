use std::{future::Future, time::Duration};

#[cfg(feature = "fuso-rt-tokio")]
pub async fn wait_for<F, O>(timeout: Duration, fut: F) -> crate::Result<O>
where
    F: Future<Output = O> + Send + 'static,
    O: Send + 'static,
{
    Ok(tokio::time::timeout(timeout, fut).await?)
}

#[cfg(feature = "fuso-rt-tokio")]
pub async fn sleep(timeout: Duration) {
    tokio::time::sleep(timeout).await
}
