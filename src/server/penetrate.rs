use std::{collections::HashMap, pin::Pin, sync::Arc, task::Poll, time::Duration};

use async_mutex::Mutex;
use std::future::Future;

use crate::{
    listener::Accepter,
    middleware::FactoryTransfer,
    protocol::{AsyncRecvPacket, AsyncSendPacket},
    service::Factory,
    Addr, Stream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub enum State<T> {
    Map(T, T),
    Close(T),
    Transferred,
}

pub enum Peer<T> {
    Visit(T, Option<Addr>),
    Client(u32, T),
}

#[derive(Default, Clone)]
pub struct WaitFor<T> {
    identify: Arc<Mutex<u32>>,
    wait_list: Arc<async_mutex::Mutex<HashMap<u32, T>>>,
}

pub struct Penetrate<T, A> {
    target: Arc<Mutex<T>>,
    factory: Arc<FactoryTransfer<Peer<T>>>,
    wait_for: WaitFor<async_channel::Sender<T>>,
    futures: Vec<BoxedFuture<State<T>>>,
    timeout: Duration,
    accepter: A,
}

impl<T> WaitFor<T> {
    pub async fn push(&self, item: T) -> u32 {
        let mut ident = self.identify.lock().await;
        let mut wait_list = self.wait_list.lock().await;
        while wait_list.contains_key(&ident) {
            let (next, overflowing) = ident.overflowing_add(1);

            if overflowing {
                *ident = 0;
            } else {
                *ident = next;
            }
        }

        wait_list.insert(*ident, item);

        *ident
    }

    pub async fn remove(&self, id: u32) -> Option<T> {
        self.wait_list.lock().await.remove(&id)
    }
}

impl<T, A> Penetrate<T, A>
where
    T: Stream + Send + 'static,
    A: Accepter<Stream = T> + Unpin + Send + 'static,
{
    pub fn new<F: Into<FactoryTransfer<Peer<T>>>>(
        factory: F,
        timeout: Duration,
        target: T,
        accepter: A,
    ) -> Self {
        let target = Arc::new(Mutex::new(target));

        let recv_fut = Self::poll_handle_recv(target.clone());
        let write_fut = Self::poll_heartbeat_future(target.clone());

        Self {
            target,
            timeout,
            accepter,
            wait_for: WaitFor {
                identify: Default::default(),
                wait_list: Default::default(),
            },
            factory: Arc::new(factory.into()),
            futures: vec![Box::pin(recv_fut), Box::pin(write_fut)],
        }
    }

    async fn poll_handle_recv<R>(stream: Arc<Mutex<T>>) -> crate::Result<R> {
        loop {
            let stream = stream.clone();
            let future = async move {
                let packet = stream.lock().await.recv_packet().await?;
                Ok::<_, crate::Error>(())
            };

            async_timer::timed(future, Duration::from_secs(10)).await??;
        }
    }

    async fn poll_heartbeat_future<R>(stream: Arc<Mutex<T>>) -> crate::Result<R> {
        loop {
            let stream = stream.clone();
            let future = async move {
                Ok::<_, crate::Error>(stream.lock().await.send_packet(b"ping").await?)
            };

            async_timer::timed(future, Duration::from_secs(10)).await??;
        }
    }
}

impl<T, A> Accepter for Penetrate<T, A>
where
    T: Stream + Send + 'static,
    A: Accepter<Stream = T> + Unpin + Send + 'static,
{
    type Stream = (T, T);

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        match Pin::new(&mut self.accepter).poll_accept(cx)? {
            Poll::Pending => {}
            Poll::Ready(stream) => {
                let target = self.target.clone();
                let factory = self.factory.clone();
                let timeout = self.timeout;
                let queue = self.wait_for.clone();
                let fut: BoxedFuture<State<T>> = Box::pin(async move {
                    match factory.call(Peer::Visit(stream, None)).await? {
                        Peer::Visit(stream, addr) => {
                            let (accept_tx, accept_ax) = async_channel::bounded(1);
                            let id = queue.push(accept_tx).await;

                            let future = async move {
                                // 通知客户端建立连接
                                // let _ = target.lock().await.send_packet(buf);

                                Ok::<_, crate::Error>(State::Map(stream, accept_ax.recv().await?))
                            };

                            let r = match async_timer::timed(future, timeout).await {
                                Ok(Ok(r)) => Ok(r),
                                Ok(Err(e)) => Err(e),
                                Err(e) => Err(e.into()),
                            };

                            match r {
                                Ok(e) => Ok(e),
                                Err(e) => {
                                    queue.remove(id).await.map(|r| r.close());
                                    Err(e)
                                }
                            }
                        }
                        Peer::Client(id, stream) => match queue.remove(id).await {
                            None => Ok(State::Close(stream)),
                            Some(sender) => {
                                sender.send(stream).await?;
                                Ok(State::Transferred)
                            }
                        },
                    }
                });
                self.futures.push(Box::pin(fut));
            }
        }

        let mut futures = Vec::new();
        while let Some(mut future) = self.futures.pop() {
            match Pin::new(&mut future).poll(cx) {
                Poll::Ready(Ok(State::Map(s1, s2))) => return Poll::Ready(Ok((s1, s2))),
                Poll::Ready(Ok(State::Close(_))) => {
                    log::warn!("Peer is closed");
                }
                Poll::Ready(Ok(State::Transferred)) => {
                    log::warn!("Transferred");
                }
                Poll::Ready(Err(e)) => {
                    log::warn!("{}", e);
                }
                Poll::Pending => futures.push(future),
            }
        }

        drop(std::mem::replace(&mut self.futures, futures));

        Poll::Pending
    }
}
