mod connector;
mod linker;
mod transport;

pub use connector::*;
pub use linker::*;

use crate::core::connector::ReplicableConnector;
use crate::core::rpc::structs::port_forward::Target;
use crate::core::transfer::AbstractTransmitter;
use crate::core::{
    connector::AbstractConnector,
    rpc::{structs::port_forward::Request, AsyncCallee},
};
use std::sync::Arc;
use std::{marker::PhantomData, pin::Pin, task::Poll};

use crate::{
    client::port_forward::transport::Transport,
    config::client::FinalTarget,
    core::{accepter::Accepter, connector::Connector, Stream},
    runtime::Runtime,
};

pub struct PortForwarder<R, S> {
    target: FinalTarget,
    transport: Transport<'static, S>,
    connector: ReplicableConnector<'static, Protocol, AbstractTransmitter<'static>>,
    _marked: PhantomData<R>,
}

impl<R, S> PortForwarder<R, S>
where
    R: Runtime + 'static,
    S: Stream + Unpin + Send + 'static,
{
    pub fn new_with_runtime<C>(transport: S, target: FinalTarget, connector: C) -> Self
    where
        C: Connector<Protocol, Output = AbstractTransmitter<'static>>,
        C: Sync + Send + Unpin + 'static,
    {
        let transport = Transport::new::<R>(std::time::Duration::from_secs(1), transport);

        Self {
            target,
            transport,
            connector: ReplicableConnector(Arc::new(AbstractConnector::new(connector))),
            _marked: PhantomData,
        }
    }
}

impl<R, S> Accepter for PortForwarder<R, S>
where
    R: Unpin,
    S: Stream + Send + Unpin,
{
    type Output = (Linker, FinalTarget);

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<Self::Output>> {
        match Pin::new(&mut self.transport).poll_next(ctx)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready((request, responder)) => match request {
                Request::Dyn(token, _) => {
                    let linker = Linker::new(token, self.connector.clone(), responder);
                    Poll::Ready(Ok((linker, FinalTarget::Dynamic)))
                }
                Request::New(token, target) => Poll::Ready(Ok({
                    let linker = Linker::new(token, self.connector.clone(), responder);
                    (linker, {
                        match target {
                            None => self.target.clone(),
                            Some(target) => target.into(),
                        }
                    })
                })),
            },
        }
    }
}

impl From<Target> for FinalTarget {
    fn from(value: Target) -> Self {
        match value {
            Target::Udp(addr, port) => FinalTarget::Udp { addr, port },
            Target::Tcp(addr, port) => FinalTarget::Tcp { addr, port },
        }
    }
}
