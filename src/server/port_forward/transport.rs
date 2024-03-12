use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use super::{Connection, Rc4MagicHandshake, Whence};
use crate::core::rpc::structs::port_forward::{Request, Response};
use crate::core::rpc::AsyncCall;
use crate::core::BoxedFuture;
use crate::core::{accepter::Accepter, BoxedStream};
use crate::error;



pub struct Transport{

}




pub struct ShareAccepter<A> {
    accepter: A,
    accepted: Vec<Whence>,
    handshaker: Arc<Rc4MagicHandshake>,
    connections: Vec<BoxedFuture<'static, error::Result<Whence>>>,
}

impl<A> Accepter for ShareAccepter<A>
where
    A: Accepter<Output = (SocketAddr, BoxedStream<'static>)> + Unpin + 'static,
{
    type Output = Whence;

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::error::Result<Self::Output>> {
        let mut polled = false;

        loop {
            polled = match self.accepted.pop() {
                Some(whence) => break Poll::Ready(Ok(whence)),
                None => match polled {
                    true => break Poll::Pending,
                    false => match Pin::new(&mut self.accepter).poll_accept(ctx)? {
                        std::task::Poll::Pending => true,
                        std::task::Poll::Ready(conn) => {
                            let handshaker = self.handshaker.clone();
                            self.connections.push(Box::pin(Self::do_discern(
                                handshaker,
                                Connection::from(conn),
                            )));

                            false
                        }
                    },
                },
            };

            let mut accepted = Vec::new();

            self.connections
                .retain_mut(|fut| match Pin::new(fut).poll(ctx) {
                    Poll::Pending => true,
                    Poll::Ready(r) => match r {
                        Ok(whence) => {
                            accepted.push(whence);
                            false
                        }
                        Err(e) => {
                            log::error!("{:?}", e);
                            false
                        }
                    },
                });

            drop(std::mem::replace(&mut self.accepted, accepted));
        }
    }
}



impl<A> ShareAccepter<A> {
    async fn do_discern(
        handshaker: Arc<Rc4MagicHandshake>,
        connection: Connection,
    ) -> error::Result<Whence> {
        
        unimplemented!()
    }
}




impl<A> ShareAccepter<A>
where
    A: Accepter<Output = (SocketAddr, BoxedStream<'static>)> + Unpin + Send,
{
    pub fn new(accepter: A, magic: u32, secret: Vec<u8>) -> Self {
        Self {
            accepter,
            accepted: Vec::new(),
            connections: Default::default(),
            handshaker: Arc::new(Rc4MagicHandshake {
                expect: magic,
                secret,
            }),
        }
    }
}



impl AsyncCall<Request> for Transport{
    type Output = error::Result<Response>;

    fn call<'a>(&'a mut self, data: Request) -> BoxedFuture<'a, Self::Output> {
        Box::pin(async move{
            unimplemented!()
        })
    }

}