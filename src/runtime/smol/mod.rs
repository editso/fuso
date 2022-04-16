use crate::traits::Executor;

#[derive(Default, Clone)]
pub struct DefaultExecutor;

impl Executor for DefaultExecutor {
    type Output = ();
    fn spawn<F>(&self, fut: F) -> Self::Output
    where
        F: std::future::Future<Output = Self::Output> + Send + 'static,
    {
        smol::spawn(fut).detach();
        ()
    }
}
