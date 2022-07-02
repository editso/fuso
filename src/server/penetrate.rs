use std::{collections::HashMap, fmt::Display, pin::Pin, sync::Arc, task::Poll, time::Duration};

use async_mutex::Mutex;
use std::future::Future;

use crate::{
    ext::AsyncWriteExt,
    generator::Generator,
    guard::{Fallback, Timer},
    io,
    listener::Accepter,
    middleware::FactoryTransfer,
    protocol::{AsyncRecvPacket, AsyncSendPacket, Message, ToVec},
    ready,
    service::{Factory, ServerFactory},
    Addr, Socket, Stream,
};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub enum State<T> {
    Stop,
    Map(T, T),
    Close(T),
    Transferred,
    Error(crate::Error),
}

pub enum Peer<T> {
    Visit(T, Socket),
    Client(u32, T),
}

#[derive(Default, Clone)]
pub struct WaitFor<T> {
    identify: Arc<Mutex<u32>>,
    wait_list: Arc<async_mutex::Mutex<HashMap<u32, T>>>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub max_wait_time: Duration,
    pub heartbeat_timeout: Duration,
    pub read_timeout: Option<Duration>,
    pub write_timeout: Option<Duration>,
    pub fallback_strict_mode: bool,
}

pub struct PenetrateFactory<T> {
    pub(crate) config: Config,
    pub(crate) factory: Arc<FactoryTransfer<Peer<Fallback<T>>>>,
}

pub struct PenetrateGenerator<T, A>(Penetrate<T, A>);

pub struct PeerFactory;

pub struct Penetrate<T, A> {
    target: Arc<Mutex<Timer<T>>>,
    config: Config,
    factory: Arc<FactoryTransfer<Peer<Fallback<T>>>>,
    wait_for: WaitFor<async_channel::Sender<Fallback<T>>>,
    futures: Vec<BoxedFuture<State<T>>>,
    accepter: A,
}

impl<T> WaitFor<T> {
    pub async fn push(&self, item: T) -> u32 {
        // FIXME cur === next may lead to an infinite loop
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
    pub fn new(
        config: Config,
        factory: Arc<FactoryTransfer<Peer<Fallback<T>>>>,
        target: T,
        accepter: A,
    ) -> Self {
        log::debug!("{}", config);

        let target = Arc::new(Mutex::new({
            Timer::new(target, config.read_timeout, config.write_timeout)
        }));

        let wait_for = WaitFor {
            identify: Default::default(),
            wait_list: Default::default(),
        };

        let recv_fut = Self::poll_handle_recv(wait_for.clone(), target.clone());
        let write_fut = Self::poll_heartbeat_future(target.clone(), config.heartbeat_timeout);

        Self {
            target,
            config,
            factory,
            accepter,
            wait_for,
            futures: vec![Box::pin(recv_fut), Box::pin(write_fut)],
        }
    }

    async fn poll_handle_recv(
        wait_for: WaitFor<async_channel::Sender<Fallback<T>>>,
        stream: Arc<Mutex<Timer<T>>>,
    ) -> crate::Result<State<T>> {
        loop {
            let packet = stream.lock().await.recv_packet().await;
            if packet.is_err() {
                let err = unsafe { packet.unwrap_err_unchecked() };
                log::warn!("client error {}", err);
                return Err(err.into());
            }

            let packet = unsafe { packet.unwrap_unchecked() };

            log::debug!("{:?}", packet);
        }
    }

    async fn poll_heartbeat_future(
        stream: Arc<Mutex<Timer<T>>>,
        timeout: Duration,
    ) -> crate::Result<State<T>> {
        loop {
            let stream = stream.clone();

            log::trace!("send heartbeat packet to client");

            if let Err(e) = stream.lock().await.send_packet(b"ping").await {
                log::warn!("failed to send heartbeat packet to client");
                break Err(e.into());
            }

            tokio::time::sleep(timeout).await;
        }
    }
}

impl<T, A> Accepter for Penetrate<T, A>
where
    T: Stream + Send + Sync + 'static,
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
                let client = self.target.clone();
                let factory = self.factory.clone();
                let timeout = self.config.max_wait_time;
                let wait_for = self.wait_for.clone();
                let fallback_strict_mode = self.config.fallback_strict_mode;
                let fut = Box::pin(async move {
                    let fallback = Fallback::new(stream, fallback_strict_mode);
                    match factory.call(Peer::Visit(fallback, Socket::Default)).await? {
                        Peer::Visit(stream, socket) => {
                            let (accept_tx, accept_ax) = async_channel::bounded(1);
                            let id = wait_for.push(accept_tx).await;

                            let message = Message::Map(id, socket).to_vec();

                            if let Err(e) = client.lock().await.send_packet(&message).await {
                                log::warn!(
                                    "notify the client that the connection establishment failed"
                                );
                                return Ok(State::Error(e));
                            }

                            let future = async move {
                                // 通知客户端建立连接

                                log::debug!("client notified, waiting for mapping");

                                let mut s1 = stream;
                                let mut s2 = accept_ax.recv().await?;

                                s1.backward().await?;

                                if let Some(data) = s1.back_data() {
                                    log::debug!("copy data to peer {}bytes", data.len());
                                    if let Err(e) = s2.write_all(data).await {
                                        log::warn!(
                                            "mapping failed, the client has closed the connection"
                                        );
                                        return Err(e.into());
                                    }
                                }

                                Ok::<_, crate::Error>(State::Map(s1.into_inner(), s2.into_inner()))
                            };

                            let r = match async_timer::timed(future, timeout).await {
                                Ok(Ok(r)) => Ok(r),
                                Ok(Err(e)) => Err(e),
                                Err(e) => {
                                    log::warn!("mapping timed out");
                                    Err(e.into())
                                }
                            };

                            match r {
                                Ok(s) => {
                                    log::debug!("mapping was established successfully");
                                    Ok(s)
                                }
                                Err(e) => {
                                    log::debug!("mapping failed, clear mapping information");
                                    wait_for.remove(id).await.map(|r| r.close());
                                    Err(e)
                                }
                            }
                        }
                        Peer::Client(id, stream) => match wait_for.remove(id).await {
                            None => {
                                log::warn!("the client established a mapping request, but the peer was closed");
                                Ok(State::Close(stream.into_inner()))
                            }
                            Some(sender) => {
                                log::warn!("mapping request established");
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
                Poll::Pending => futures.push(future),
                Poll::Ready(Ok(State::Map(s1, s2))) => return Poll::Ready(Ok((s1, s2))),
                Poll::Ready(Ok(State::Stop)) => {
                    log::warn!("client closes connection");
                    return Poll::Ready(Err(crate::error::Kind::Channel.into()));
                }
                Poll::Ready(Ok(State::Error(e))) => {
                    log::warn!("client error {}", e);
                    return Poll::Ready(Err(e));
                }
                Poll::Ready(Ok(State::Close(_))) => {
                    log::warn!("Peer is closed");
                }
                Poll::Ready(Ok(State::Transferred)) => {
                    log::warn!("Transferred");
                }
                Poll::Ready(Err(e)) => {
                    log::warn!("encountered other errors {}", e);
                }
            }
        }

        drop(std::mem::replace(&mut self.futures, futures));

        Poll::Pending
    }
}

impl<SF, CF, A, S> Factory<(ServerFactory<SF, CF>, S)> for PenetrateFactory<S>
where
    SF: Factory<Addr, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Addr, Output = BoxedFuture<S>> + Send + Sync + 'static,
    A: Accepter<Stream = S> + Send + Unpin + 'static,
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<PenetrateGenerator<S, A>>;

    fn call(&self, (factory, mut target): (ServerFactory<SF, CF>, S)) -> Self::Output {
        let peer_factory = self.factory.clone();
        let config = self.config.clone();

        Box::pin(async move {
            match target.recv_packet().await {
                Err(e) => {
                    log::warn!("client error {}", e);
                    return Err(e);
                }
                Ok(packet) => {
                    let accepter = factory.bind(9999).await?;
                    log::debug!("start listening");

                    Ok(PenetrateGenerator(Penetrate::new(
                        config,
                        peer_factory,
                        target,
                        accepter,
                    )))
                }
            }
        })
    }
}

impl<T, A> Generator for PenetrateGenerator<T, A>
where
    A: Accepter<Stream = T> + Send + Unpin + 'static,
    T: Stream + Send + Sync + 'static,
{
    type Output = Option<BoxedFuture<()>>;

    fn poll_generate(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> Poll<crate::Result<Self::Output>> {
        let (s1, s2) = ready!(Pin::new(&mut self.0).poll_accept(cx)?);
        Poll::Ready(Ok(Some(Box::pin(async move {
            let results = io::forward(s1, s2).await;
            log::debug!("{:?}", results);
            Ok(())
        }))))
    }
}

impl<S> Factory<Peer<Fallback<S>>> for PeerFactory
where
    S: Stream + Send + 'static,
{
    type Output = BoxedFuture<Peer<Fallback<S>>>;

    fn call(&self, cfg: Peer<Fallback<S>>) -> Self::Output {
        Box::pin(async move {
            match cfg {
                Peer::Visit(mut s, _) => {
                    let _ = s.backward().await;
                    let a = s.recv_packet().await;
                    Ok(Peer::Visit(s, Socket::Default))
                }
                Peer::Client(_, _) => todo!(),
            }
        })
    }
}

impl Display for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "read_timeout = {:?}, write_timeout = {:?}, max_wait_time={:?}, heartbeat_time={:?}",
            self.read_timeout, self.write_timeout, self.max_wait_time, self.heartbeat_timeout
        )
    }
}
