use std::future::Future;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use super::{Connection, Rc4MagicHandshake, Whence};
use crate::core::future::Poller;
use crate::core::{accepter::Accepter, BoxedStream};
use crate::error;
use crate::runtime::Runtime;

pub struct ShareAccepter<R, A> {
    accepter: A,
    handshaker: Arc<Rc4MagicHandshake>,
    connections: Poller<'static, error::Result<Whence>>,
    _marked: PhantomData<R>,
}

impl<R, A> Accepter for ShareAccepter<R, A>
where
    A: Accepter<Output = (SocketAddr, BoxedStream<'static>)> + Unpin + 'static,
    R: Runtime + Unpin + 'static,
{
    type Output = Whence;

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<Self::Output>> {
        let mut polled = false;

        while !polled {
            polled = match Pin::new(&mut self.accepter).poll_accept(ctx)? {
                std::task::Poll::Pending => true,
                std::task::Poll::Ready(conn) => {
                    let handshaker = self.handshaker.clone();
                    let fut = R::wait_for(
                        std::time::Duration::from_secs(10),
                        Self::do_discern(handshaker, Connection::from(conn)),
                    );
                    self.connections.add(async move { fut.await? });
                    false
                }
            };

            if let Poll::Ready(r) = Pin::new(&mut self.connections).poll(ctx) {
                return Poll::Ready(r);
            }
        }

        Poll::Pending
    }
}


impl<R, A> ShareAccepter<R, A> {
    async fn do_discern(
        handshaker: Arc<Rc4MagicHandshake>,
        connection: Connection,
    ) -> error::Result<Whence> {
        
        

        unimplemented!()
    }
}


#[cfg(feature = "fuso-runtime")]
impl<R, A> ShareAccepter<R, A>
where
    A: Accepter<Output = (SocketAddr, BoxedStream<'static>)> + Unpin + Send,
{
    pub fn new_runtime(accepter: A, magic: u32, secret: Vec<u8>) -> Self {
        Self {
            accepter,
            connections: Poller::new(),
            handshaker: Arc::new(Rc4MagicHandshake {
                expect: magic,
                secret,
            }),
            _marked: PhantomData,
        }
    }
}
