mod handshake;
mod transport;

pub use handshake::*;
pub use transport::*;

use parking_lot::Mutex;

use std::{collections::HashMap, net::SocketAddr, pin::Pin, sync::Arc, task::Poll};

use crate::{
    core::{
        accepter::Accepter,
        io::{AsyncRead, AsyncWrite},
        processor::{BoxedPreprocessor, Preprocessor},
        protocol::AsyncPacketSend,
        rpc::{
            structs::port_forward::{Request, VisitorProtocol},
            AsyncCall,
        },
        split::WriteHalf,
        BoxedStream,
    },
    error,
};

type Connection = crate::core::Connection<'static>;

enum Forward {
    Ready(u64, Connection),
    Pending(u64),
}

pub enum Whence {
    Visitor(Connection),
    Transport(Connection),
}

#[derive(Clone)]
pub struct Visitors {
    connections: Arc<Mutex<HashMap<u64, Connection>>>,
}

pub struct PortForwarder<A, T> {
    accepter: A,
    transport: T,
    visitors: Visitors,
    preprocessor: BoxedPreprocessor<'static, Connection, VisitorProtocol>,
}


impl<A, T> Accepter for PortForwarder<A, T>
where
    A: Accepter<Output = Whence> + Unpin,
    T: Unpin,
{
    type Output = (Connection, Connection);

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<Self::Output>> {
        let poll = Pin::new(&mut self.accepter).poll_accept(ctx)?;

        match poll {
            std::task::Poll::Pending => return Poll::Pending,
            std::task::Poll::Ready(whence) => match whence {
                Whence::Visitor(visitor) => {
                    // let preprocessor = self.preprocessor.clone();
                    // let protocol = preprocessor.prepare(visitor).await;

                    // match protocol {
                    //     VisitorProtocol::Socks(conn, socks) => {
                            
                    //     },
                    //     VisitorProtocol::Other(conn, _) => {

                    //     },
                    // }

                    // Self::new_transport();
                    unimplemented!()
                }
                Whence::Transport(transport) => {
                    // Self::handshake_transport(transport);
                    unimplemented!()
                }
            },
        }
    }
}



impl<A, T> PortForwarder<A, T>
where
    A: Accepter<Output = Whence>,
    T: AsyncRead + AsyncWrite,
{
    pub fn new(transport: T, accepter: A) -> Self {
        // Self {
        //     accepter,
        //     transport,
        // }
        unimplemented!()
    }
}

impl<A, T> PortForwarder<A, T> {
    async fn new_transport(transport: Transport, token: u64) -> error::Result<u64> {
        // let a = transport.call(Request::New(token, None)).await;

        // let a = transport.send_packet(NewTransport()).await;

        unimplemented!()
    }
}
