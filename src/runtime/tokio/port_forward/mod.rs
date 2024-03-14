use std::net::SocketAddr;

use crate::{
    core::{
        accepter::Accepter,
        io::{AsyncRead, AsyncWrite},
        processor::Preprocessor,
        rpc::structs::port_forward::VisitorProtocol,
        BoxedStream, Connection,
    },
    server::port_forward::{PortForwarder, ShareAccepter, Whence},
};

use super::TokioRuntime;

impl<A> ShareAccepter<TokioRuntime, A>
where
    A: Accepter<Output = (SocketAddr, BoxedStream<'static>)> + Unpin + Send,
{
    pub fn new(accepter: A, magic: u32, secret: Vec<u8>) -> Self {
        ShareAccepter::<TokioRuntime, A>::new_runtime(accepter, magic, secret)
    }
}

impl<A, T> PortForwarder<TokioRuntime, A, T>
where
    A: Accepter<Output = Whence> + Unpin + 'static,
    T: AsyncRead + AsyncWrite + Unpin + 'static,
{
    pub fn new<P>(stream: T, accepter: A, preprocessor: P) -> Self
    where
        P: Preprocessor<Connection<'static>, Output = VisitorProtocol> + Send + Sync + 'static,
    {
        PortForwarder::<TokioRuntime, A, T>::new_with_runtime(stream, accepter, preprocessor)
    }
}
