use std::net::SocketAddr;

use crate::{
    client::port_forward::Protocol,
    config::client::FinalTarget,
    core::{
        accepter::Accepter,
        connector::Connector,
        processor::{AbstractPreprocessor, Preprocessor},
        rpc::structs::port_forward::VisitorProtocol,
        stream::handshake::MuxConfig,
        transfer::AbstractTransmitter,
        AbstractStream, Connection, Stream,
    },
    error,
    server::port_forward::{MuxAccepter, Whence},
};

use super::TokioRuntime;

impl<A> MuxAccepter<TokioRuntime, A>
where
    A: Accepter<Output = (SocketAddr, AbstractStream<'static>)> + Unpin + Send,
{
    pub fn new(conf: MuxConfig, accepter: A) -> Self {
        Self::new_runtime(accepter, conf)
    }
}

impl<A, T> crate::server::port_forward::PortForwarder<TokioRuntime, A, T>
where
    A: Accepter<Output = Whence> + Unpin + 'static,
    T: Stream + Send + Unpin + 'static,
{
    pub fn new<P, M>(transport: T, accepter: A, prepvis: P, prepmap: M) -> Self
    where
        P: Into<AbstractPreprocessor<'static, Connection<'static>, error::Result<VisitorProtocol>>>,
        M: Preprocessor<Connection<'static>, Output = error::Result<Connection<'static>>>,
        M: Send + Sync + 'static,
    {
        Self::new_with_runtime(transport, accepter, prepvis, prepmap)
    }
}

impl<S> crate::client::port_forward::PortForwarder<TokioRuntime, S>
where
    S: Stream + Send + Unpin + 'static,
{
    pub fn new<C>(transport: S, target: FinalTarget, connector: C) -> Self
    where
        C: Connector<Protocol, Output = AbstractTransmitter<'static>>,
        C: Sync + Send + Unpin + 'static,
    {
        Self::new_with_runtime(transport, target, connector)
    }
}
