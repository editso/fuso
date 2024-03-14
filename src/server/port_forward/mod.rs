mod accepter;
mod handshake;
mod preprocessor;
mod transport;

pub use accepter::*;
pub use handshake::*;
pub use preprocessor::*;
pub use transport::*;

use parking_lot::Mutex;
use std::future::Future;

use std::marker::PhantomData;
use std::{collections::HashMap, pin::Pin, sync::Arc, task::Poll};

use crate::core::future::Poller;
use crate::core::rpc::structs::port_forward;
use crate::runtime::Runtime;
use crate::{
    core::{
        accepter::Accepter,
        io::{AsyncRead, AsyncWrite},
        processor::{BoxedPreprocessor, Preprocessor},
        rpc::{
            structs::port_forward::{Request, VisitorProtocol},
            AsyncCall,
        },
        split::SplitStream,
    },
    error,
};

type Connection = crate::core::Connection<'static>;

enum Outcome {
    Ready(u64, Connection),
    Pending(u64),
    Timeout(u64),
}

pub enum Whence {
    Visitor(Connection),
    Transport(Connection),
}

#[derive(Clone)]
pub struct Visitors {
    inc_token: Arc<Mutex<u64>>,
    connections: Arc<Mutex<HashMap<u64, Connection>>>,
}

pub struct PortForwarder<Runtime, A, T> {
    poller: Poller<'static, error::Result<Outcome>>,
    accepter: A,
    visitors: Visitors,
    transport: Transport<T>,
    preprocessor: BoxedPreprocessor<'static, Connection, VisitorProtocol>,
    _marked: PhantomData<Runtime>,
}

impl<R, A, T> Accepter for PortForwarder<R, A, T>
where
    A: Accepter<Output = Whence> + Unpin + 'static,
    T: Unpin + Send + 'static,
    R: Runtime + Unpin + 'static,
{
    type Output = (Connection, Connection);

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<Self::Output>> {
        let mut polled = false;

        while !polled {
            let whence = match Pin::new(&mut self.accepter).poll_accept(ctx)? {
                std::task::Poll::Pending => None,
                std::task::Poll::Ready(whence) => Some(whence),
            };

            polled = match whence {
                None => true,
                Some(Whence::Visitor(conn)) => {
                    let fut = Self::do_prepare_visitor(
                        conn,
                        self.visitors.clone(),
                        self.transport.clone(),
                        self.preprocessor.clone(),
                    );

                    self.poller.add(fut);

                    false
                }
                Some(Whence::Transport(conn)) => {
                    let fut = Self::new_transport(conn, self.transport.clone());
                    self.poller.add(fut);

                    false
                }
            };

            if let Poll::Ready(r) = Pin::new(&mut self.poller).poll(ctx) {
                match r {
                    Err(_) => unimplemented!(),
                    Ok(forward) => match forward {
                        Outcome::Pending(token) => {
                            self.poller.add(Self::wait_transport(
                                token,
                                std::time::Duration::from_secs(10),
                            ));
                        }
                        Outcome::Timeout(token) => match self.visitors.take(token) {
                            Some(_) => {}
                            None => {}
                        },
                        Outcome::Ready(token, transport) => {
                            match self.visitors.take(token) {
                                Some(visitor) => return Poll::Ready(Ok((visitor, transport))),
                                None => {}
                            };
                        }
                    },
                }
            }
        }

        Poll::Pending
    }
}


#[cfg(feature = "fuso-runtime")]
impl<R, A, T> PortForwarder<R, A, T>
where
    A: Accepter<Output = Whence> + Unpin + 'static,
    T: AsyncRead + AsyncWrite + Unpin + 'static,
{
    pub fn new_with_runtime<P>(stream: T, accepter: A, preprocessor: P) -> Self
    where
        P: Preprocessor<Connection, Output = VisitorProtocol> + Send + Sync + 'static,
    {
        let (reader, writer) = stream.split();
        Self {
            poller: Poller::new(),
            accepter,
            visitors: Default::default(),
            transport: Transport::new(reader, writer),
            preprocessor: BoxedPreprocessor(Arc::new(preprocessor)),
            _marked: PhantomData,
        }
    }
}

impl<R, A, T> PortForwarder<R, A, T>
where
    R: Runtime,
{
    async fn wait_transport(token: u64, timeout: std::time::Duration) -> error::Result<Outcome> {
        R::sleep(timeout).await;
        Ok(Outcome::Timeout(token))
    }

    async fn do_prepare_visitor(
        conn: Connection,
        visitors: Visitors,
        transport: Transport<T>,
        preprocessor: BoxedPreprocessor<'_, Connection, VisitorProtocol>,
    ) -> error::Result<Outcome> {
        let mut transport = transport;

        let (conn, addr) = match preprocessor.prepare(conn).await {
            VisitorProtocol::Other(conn, addr) => (conn, addr),
            VisitorProtocol::Socks(conn, socks) => match socks {
                port_forward::WithSocks::Tcp(addr) => (conn, Some(addr)),
                port_forward::WithSocks::Udp() => todo!(),
            },
        };

        let token = visitors.store(conn);

        match transport.call(Request::New(token, addr)).await {
            Err(_) => {}
            Ok(resp) => match resp {
                port_forward::Response::Ok => {}
                port_forward::Response::Error() => todo!(),
            },
        }

        Ok(Outcome::Pending(token))
    }

    async fn new_transport(conn: Connection, transport: Transport<T>) -> error::Result<Outcome> {
        Ok(Outcome::Ready(0, conn))
    }
}


impl Default for Visitors {
    fn default() -> Self {
        Self {
            inc_token: Arc::new(Mutex::new(1)),
            connections: Default::default(),
        }
    }
}

impl Visitors {
    fn take(&self, token: u64) -> Option<Connection> {
        self.connections.lock().remove(&token)
    }

    fn store(&self, conn: Connection) -> u64 {
        let token = self.next_token();
        self.connections.lock().insert(token, conn);
        token
    }

    fn next_token(&self) -> u64 {
        let connections = self.connections.lock();
        let mut inc_token = self.inc_token.lock();

        loop {
            let cur_tok = *inc_token;

            let (cur_tok, overflow) = if connections.contains_key(&cur_tok) {
                cur_tok.overflowing_add(1)
            } else {
                (cur_tok, false)
            };

            if overflow {
                *inc_token = 1;
            } else {
                break cur_tok;
            }
        }
    }
}
