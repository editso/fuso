use std::pin::Pin;

use crate::{
    guard::Fallback,
    penetrate::{
        server::{Peer, Visitor},
        Adapter,
    },
    Provider, Socket, Stream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct DirectConverter;

impl<S> Provider<Fallback<S>> for DirectConverter
where
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<Adapter<S>>;

    fn call(&self, stream: Fallback<S>) -> Self::Output {
        Box::pin(async move {
            Ok(Adapter::Accept(Peer::Route(
                Visitor::Route(stream),
                Socket::default(),
            )))
        })
    }
}
