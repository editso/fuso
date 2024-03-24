use std::pin::Pin;
use std::task::Poll;

use super::Outcome;

use crate::core::rpc::caller::{Caller, Reactor};
use crate::core::rpc::{self, Decoder};

use crate::{
    core::{
        rpc::{
            structs::port_forward::{Request, Response},
            AsyncCall,
        },
        BoxedFuture, Stream,
    },
    error,
};

pub struct Transport<'a, T> {
    caller: Caller<'a, T>,
}

pub struct TransportHold(Reactor<'static>);

impl<'a, T> Transport<'a, T>
where
    T: Stream + Send + Unpin + 'static,
{
    pub fn new(stream: T) -> (Self, TransportHold) {
        let (reactor, caller) = rpc::caller::Caller::new(stream);
        (Self { caller }, TransportHold(reactor))
    }
}

impl<'transport, T> AsyncCall<Request> for Transport<'transport, T>
where
    T: Stream + Send + Unpin + 'transport,
{
    type Output = error::Result<Response>;

    fn call<'a>(&'a mut self, data: Request) -> BoxedFuture<'a, Self::Output> {
        Box::pin(async move { self.caller.call(data).await?.decode() })
    }
}

impl<'transport, T> Clone for Transport<'transport, T> {
    fn clone(&self) -> Self {
        Self {
            caller: self.caller.clone(),
        }
    }
}

impl std::future::Future for TransportHold {
    type Output = error::Result<Outcome>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match Pin::new(&mut self.0).poll(cx) {
            std::task::Poll::Pending => Poll::Pending,
            std::task::Poll::Ready(e) => match e {
                Err(e) => Poll::Ready(Ok(Outcome::Stopped(Some(e)))),
                Ok(()) => Poll::Ready(Ok(Outcome::Stopped(None))),
            },
        }
    }
}
