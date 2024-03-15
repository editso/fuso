use std::future::Future;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use rc4::{KeyInit, Rc4, StreamCipher};

use super::{Connection, Rc4MagicHandshake, Whence};
use crate::core::future::Poller;
use crate::core::io::AsyncReadExt;
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
                    self.connections.add(async move {
                        let mut connection = Connection::from(conn);

                        connection.mark();

                        let r = R::wait_for(
                            std::time::Duration::from_secs(10),
                            Self::try_handshake(handshaker, &mut connection),
                        )
                        .await;

                        match r {
                            Ok(Ok(true)) => {
                                connection.discard();
                                Ok(Whence::Mapping(connection))
                            }
                            _ => {
                                connection.reset();
                                Ok(Whence::Visitor(connection))
                            }
                        }
                    });

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

impl<R, A> ShareAccepter<R, A>
where
    R: Runtime,
{
    async fn try_handshake(
        handshaker: Arc<Rc4MagicHandshake>,
        connection: &mut Connection,
    ) -> error::Result<bool> {
        let mut buf = [0u8; 4];

        connection.read_exact(&mut buf).await?;

        Rc4::new((&handshaker.secret).into())
            .try_apply_keystream(&mut buf)
            .map(|()| handshaker.expect.eq(&u32::from_be_bytes(buf)))
            .map_or_else(|_| Ok(false), |z| Ok(z))
    }
}

#[cfg(feature = "fuso-runtime")]
impl<R, A> ShareAccepter<R, A>
where
    A: Accepter<Output = (SocketAddr, BoxedStream<'static>)> + Unpin + Send,
{
    pub fn new_runtime(accepter: A, magic: u32, secret: [u8; 16]) -> Self {
        Self {
            accepter,
            connections: Poller::new(),
            handshaker: Arc::new(Rc4MagicHandshake {
                secret,
                expect: magic,
            }),
            _marked: PhantomData,
        }
    }
}
