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
use crate::core::stream::handshake::MuxConfig;
use crate::core::Stream;
use crate::core::{accepter::Accepter, AbstractStream};
use crate::error;
use crate::runtime::Runtime;

enum AcceptWhence {
    Visitor,
    Mapping,
}

pub struct MuxAccepter<R, A> {
    accepter: A,
    handshaker: Arc<Rc4MagicHandshake>,
    connections: Poller<'static, error::Result<Whence>>,
    _marked: PhantomData<R>,
}

pub struct ForwardAccepter<A> {
    whence: AcceptWhence,
    accepter: A,
}

impl<R, A> Accepter for MuxAccepter<R, A>
where
    A: Accepter<Output = (SocketAddr, AbstractStream<'static>)> + Unpin + 'static,
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
                            std::time::Duration::from_millis(1000),
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

impl<R, A> MuxAccepter<R, A>
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
            .map(|()| handshaker.expect.eq(&u32::from_le_bytes(buf)))
            .map_or_else(|_| Ok(false), |z| Ok(z))
    }
}

#[cfg(feature = "fuso-runtime")]
impl<R, A> MuxAccepter<R, A>
where
    A: Accepter<Output = (SocketAddr, AbstractStream<'static>)> + Unpin + Send,
{
    pub fn new_runtime(accepter: A, conf: MuxConfig) -> Self {
        Self {
            accepter,
            connections: Poller::new(),
            handshaker: Arc::new(Rc4MagicHandshake {
                expect: conf.magic,
                secret: conf.secret,
            }),
            _marked: PhantomData,
        }
    }
}

impl<A, T> ForwardAccepter<A>
where
    A: Accepter<Output = (SocketAddr, T)> + Unpin + Send,
    T: Stream + Unpin + 'static,
{
    pub fn new_visitor(accepter: A) -> Self {
        Self {
            accepter,
            whence: AcceptWhence::Visitor,
        }
    }

    pub fn new_mapping(accepter: A) -> Self {
        Self {
            accepter,
            whence: AcceptWhence::Mapping,
        }
    }
}

impl<A, T> Accepter for ForwardAccepter<A>
where
    A: Accepter<Output = (SocketAddr, T)> + Unpin + Send,
    T: Stream + Unpin + Send + 'static,
{
    type Output = Whence;

    fn poll_accept(
        mut self: Pin<&mut Self>,
        ctx: &mut std::task::Context<'_>,
    ) -> Poll<error::Result<Self::Output>> {
        match Pin::new(&mut self.accepter).poll_accept(ctx)? {
            Poll::Pending => Poll::Pending,
            Poll::Ready((addr, stream)) => Poll::Ready(Ok({
                match self.whence {
                    AcceptWhence::Visitor => {
                        Whence::Visitor(Connection::new(addr, AbstractStream::new(stream)))
                    }
                    AcceptWhence::Mapping => {
                        Whence::Mapping(Connection::new(addr, AbstractStream::new(stream)))
                    }
                }
            })),
        }
    }
}
