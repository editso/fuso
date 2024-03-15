use std::net::SocketAddr;

use crate::{
    core::{
        accepter::Accepter,
        io::{AsyncRead, AsyncWrite},
        processor::Preprocessor,
        rpc::structs::port_forward::VisitorProtocol,
        BoxedStream, Connection, Stream,
    },
    error,
    server::port_forward::{PortForwarder, ShareAccepter, Whence},
};

use super::TokioRuntime;

impl<A> ShareAccepter<TokioRuntime, A>
where
    A: Accepter<Output = (SocketAddr, BoxedStream<'static>)> + Unpin + Send,
{
    pub fn new(magic: u32, secret: [u8; 16], accepter: A) -> Self {
        ShareAccepter::<TokioRuntime, A>::new_runtime(accepter, magic, secret)
    }
}

impl<A, T> PortForwarder<TokioRuntime, A, T>
where
    A: Accepter<Output = Whence> + Unpin + 'static,
    T: Stream + Send + Unpin + 'static,
{
    pub fn new<P, M>(stream: T, accepter: A, prepvis: P, prepmap: M) -> Self
    where
        P: Preprocessor<Connection<'static>, Output = error::Result<VisitorProtocol>>,
        P: Send + Sync + 'static,
        M: Preprocessor<Connection<'static>, Output = error::Result<Connection<'static>>>,
        M: Send + Sync + 'static,
    {
        PortForwarder::<TokioRuntime, A, T>::new_with_runtime(stream, accepter, prepvis, prepmap)
    }
}
