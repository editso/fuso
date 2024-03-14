mod kcp;
mod tcp;

mod port_forward;

pub use kcp::*;
pub use tcp::*;

pub use port_forward::*;

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

    fn wait_for<F, O>(
        timeout: std::time::Duration,
        future: F,
    ) -> crate::core::BoxedFuture<'static, crate::error::Result<O>>
    where
        F: std::future::Future<Output = O> + Send + 'static,
    {
        Box::pin(async move {
            let a = tokio::time::timeout(timeout, future).await;
            unimplemented!()
        })
    }
    
    fn sleep(timeout: std::time::Duration) -> crate::core::BoxedFuture<'static, ()> {
        todo!()
    }
}
