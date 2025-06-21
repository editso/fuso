use std::future::Future;

use crate::error;

pub struct TaskPool {}

impl Future for TaskPool {
    type Output = error::Result<()>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        unimplemented!()
    }
}
