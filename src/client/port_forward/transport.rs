use std::{future::Future, marker::PhantomData, pin::Pin};

use crate::{
    core::{
        rpc::{
            self, structs::port_forward::Request, AsyncCallee, Callee, Decoder, Looper,
            Responder,
        },
        Stream,
    },
    error,
    runtime::Runtime,
};

pub struct Transport<'transport, S> {
    callee: Callee<'transport>,
    lopper: Looper<'transport>,
    _marked: PhantomData<S>,
}

impl<'transport, S> Transport<'transport, S>
where
    S: Stream + Send + Unpin + 'transport,
{
    pub fn new<R>(heartbeat_delay: std::time::Duration, stream: S) -> Self
    where
        R: Runtime,
    {
        let (lopper, callee) = rpc::Callee::new(stream, heartbeat_delay);

        Self {
            callee,
            lopper,
            _marked: PhantomData,
        }
    }
}

impl<'transport, S> AsyncCallee for Transport<'transport, S>
where
    S: Stream + Send + Unpin,
{
    type Output = error::Result<(Request, Responder)>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match Pin::new(&mut self.lopper).poll(cx)? {
            std::task::Poll::Ready(()) => std::task::Poll::Ready(Err(error::FusoError::Abort)),
            std::task::Poll::Pending => match Pin::new(&mut self.callee).poll_next(cx)? {
                std::task::Poll::Pending => std::task::Poll::Pending,
                std::task::Poll::Ready((packet, responder)) => {
                    std::task::Poll::Ready(Ok((packet.decode()?, responder)))
                }
            },
        }
    }
}
