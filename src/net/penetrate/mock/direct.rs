use std::{pin::Pin, sync::Arc};

use crate::{
    guard::Fallback,
    penetrate::{
        server::{Peer, Visitor},
        Selector,
    },
    Provider, Socket, Stream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub struct DirectMock;

impl<S> Provider<(Fallback<S>, Arc<super::super::server::Config>)> for DirectMock
where
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<Selector<S>>;

    fn call(&self, (stream, _): (Fallback<S>, Arc<super::super::server::Config>)) -> Self::Output {
        Box::pin(async move {
            Ok(Selector::Checked(Peer::Route(
                Visitor::Route(stream),
                Socket::default(),
            )))
        })
    }
}
