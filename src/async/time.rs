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

#[cfg(feature = "fuso-rt-smol")]
pub async fn wait_for<F, O>(timeout: Duration, fut: F) -> crate::Result<O>
where
    F: Future<Output = O> + Send + 'static,
    O: Send + 'static,
{
    use crate::error;

    smol::future::race(async move { Ok(fut.await) }, async move {
        Err(error::Kind::Timeout(smol::Timer::after(timeout).await).into())
    })
    .await
}

#[cfg(feature = "fuso-rt-smol")]
pub async fn sleep(timeout: Duration) {
    let _ = smol::Timer::after(timeout).await;
}
