pub mod tokio;

pub trait Runtime {
    fn spawn<F, O>(fut: F) -> ()
    where
        F: std::future::Future<Output = O> + Send + 'static,
        O: Send + 'static;
}
