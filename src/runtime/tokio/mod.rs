mod kcp;
mod tcp;

pub use kcp::*;
pub use tcp::*;

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
}
