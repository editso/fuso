use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{guard::Fallback, request::Request, Executor};

pub enum Outcome {
    Agree,
    Reject,
}

pub trait Handler {
    type Stream;
    type Executor: Executor;

    fn can_handle<'a>(
        &self,
        stream: &'a mut Fallback<Self::Stream>,
    ) -> Pin<Box<dyn Future<Output = crate::Result<Outcome>> + Send + 'a>>;

    fn handle(
        &self,
        req: Request<Self::Stream>,
        executor: Self::Executor,
    ) -> Pin<Box<dyn Future<Output = crate::Result<()>> + Send + 'static>>;
}
