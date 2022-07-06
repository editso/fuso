use std::{collections::HashMap, fmt::Display, pin::Pin, sync::Arc, task::Poll, time::Duration};

use std::future::Future;
use tokio::sync::Mutex;

use crate::io::{ReadHalf, WriteHalf};

use crate::{
    ext::AsyncWriteExt,
    factory::FactoryWrapper,
    generator::Generator,
    guard::Fallback,
    io,
    listener::Accepter,
    protocol::{AsyncRecvPacket, AsyncSendPacket, Bind, Message, ToPacket, TryToMessage},
    ready,
    service::{Factory, ServerFactory},
    Socket, Stream,
};

use super::unpacker::Unpacker;
use crate::{time, Kind};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

pub enum PenetrateOutcome<T> {
    Map(T, T),
    Customize(BoxedFuture<()>),
}

pub enum State<T> {
    Stop,
    Close(T),
    Forward(T, T),
    Consume(BoxedFuture<()>),
    Transferred,
    Error(crate::Error),
}

pub enum Visitor<T> {
    Forward(T),
    Consume(FactoryWrapper<T, ()>),
}

pub struct PenetrateGenerator<T, A>(Penetrate<T, A>);

pub enum Peer<T> {
    Mapper(u32, T),
    Visitor(Visitor<T>, Socket),
    Unknown(T),
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
    pub(crate) unpacker: Arc<Unpacker<T>>,
}

pub struct Penetrate<T, A> {
    writer: WriteHalf<T>,
    config: Config,
    unpacker: Arc<Unpacker<T>>,
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
    T: Stream + Sync + Send + 'static,
    A: Accepter<Stream = T> + Unpin + Send + 'static,
{
    pub fn new(config: Config, unpacker: Arc<Unpacker<T>>, target: T, accepter: A) -> Self {
        log::debug!("{}", config);

        let (reader, writer) = crate::io::split(target);

        let wait_for = WaitFor {
            identify: Default::default(),
            wait_list: Default::default(),
        };

        let recv_fut = Self::poll_handle_recv(wait_for.clone(), reader.clone());
        let write_fut = Self::poll_heartbeat_future(writer.clone(), config.heartbeat_timeout);

        Self {
            writer,
            config,
            unpacker,
            accepter,
            wait_for,
            futures: vec![Box::pin(recv_fut), Box::pin(write_fut)],
        }
    }

    async fn poll_handle_recv(
        wait_for: WaitFor<async_channel::Sender<Fallback<T>>>,
        mut stream: ReadHalf<T>,
    ) -> crate::Result<State<T>> {
        loop {
            let packet = stream.recv_packet().await;

            if packet.is_err() {
                let err = unsafe { packet.unwrap_err_unchecked() };
                log::warn!("client error {}", err);
                return Ok(State::Error(err));
            }

            let packet = unsafe { packet.unwrap_unchecked() }.try_message();

            if packet.is_err() {
                log::warn!("The client sent an invalid packet");
                return Ok(State::Error(unsafe { packet.unwrap_err_unchecked() }));
            }

            let message = unsafe { packet.unwrap_unchecked() };

            match message {
                Message::Ping => {
                    log::debug!("client ping received");
                }
                Message::MapError(id, err) => {
                    log::warn!("client mapping failed, msg = {}", err);
                    wait_for.remove(id).await.map(|r| r.close());
                }
                message => {
                    log::warn!("Ignore client message {:?}", message);
                }
            }
        }
    }

    async fn poll_heartbeat_future(
        mut stream: WriteHalf<T>,
        timeout: Duration,
    ) -> crate::Result<State<T>> {
        let ping = Message::Ping.to_packet_vec();

        loop {
            log::trace!("send heartbeat packet to client");

            if let Err(e) = stream.send_packet(&ping).await {
                log::warn!("failed to send heartbeat packet to client");
                break Ok(State::Error(e));
            }

            time::sleep(timeout).await;
        }
    }

    fn async_handle(self: &mut Pin<&mut Self>, stream: T) -> BoxedFuture<State<T>> {
        let mut writer = self.writer.clone();
        let factory = self.unpacker.clone();
        let timeout = self.config.max_wait_time;
        let wait_for = self.wait_for.clone();
        let fallback_strict_mode = self.config.fallback_strict_mode;

        let fut = async move {
            let mut fallback = Fallback::new(stream, fallback_strict_mode);
            let _ = fallback.mark().await?;
            match factory.call(fallback).await? {
                Peer::Visitor(visit, socket) => {
                    let (accept_tx, accept_ax) = async_channel::bounded(1);
                    let id = wait_for.push(accept_tx).await;

                    let future = async move {
                        // 通知客户端建立连接

                        let message = Message::Map(id, socket).to_packet_vec();

                        log::debug!("notify the client to create the mapping");

                        if let Err(e) = writer.send_packet(&message).await {
                            log::warn!(
                                "notify the client that the connection establishment failed"
                            );
                            return Ok(State::Error(e));
                        }

                        log::debug!("client notified, waiting for mapping");

                        match visit {
                            Visitor::Forward(stream) => {
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

                                Ok::<_, crate::Error>(State::Forward(
                                    s1.into_inner(),
                                    s2.into_inner(),
                                ))
                            }
                            Visitor::Consume(factory) => {
                                Ok(State::Consume(factory.call(accept_ax.recv().await?)))
                            }
                        }
                    };

                    let r = match time::wait_for(timeout, future).await {
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
                Peer::Mapper(id, stream) => match wait_for.remove(id).await {
                    None => {
                        log::warn!(
                            "the client established a mapping request, but the peer was closed"
                        );
                        Ok(State::Close(stream.into_inner()))
                    }
                    Some(sender) => {
                        log::warn!("mapping request established");
                        sender.send(stream).await?;
                        Ok(State::Transferred)
                    }
                },
                Peer::Unknown(s) => {
                    log::warn!("illegal connection");
                    Ok(State::Close(s.into_inner()))
                }
            }
        };

        Box::pin(fut)
    }
}

impl<T, A> Accepter for Penetrate<T, A>
where
    T: Stream + Send + Sync + 'static,
    A: Accepter<Stream = T> + Unpin + Send + 'static,
{
    type Stream = PenetrateOutcome<T>;

    fn poll_accept(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<crate::Result<Self::Stream>> {
        let mut futures = std::mem::replace(&mut self.futures, Default::default());

        // 至少进行一次轮询，
        // accept如果就绪后没有再次poll，将会导致对端连接卡顿，甚至最坏的情况下可能无法建立连接
        let mut poll_accepter = true;

        while poll_accepter {
            poll_accepter = match Pin::new(&mut self.accepter).poll_accept(cx)? {
                Poll::Pending => false,
                Poll::Ready(stream) => {
                    futures.push(self.async_handle(stream));
                    true
                }
            };

            while let Some(mut future) = futures.pop() {
                match Pin::new(&mut future).poll(cx) {
                    Poll::Pending => {
                        self.futures.push(future);
                    }
                    Poll::Ready(Ok(State::Forward(s1, s2))) => {
                        self.futures.extend(futures);
                        return Poll::Ready(Ok::<_, crate::Error>(PenetrateOutcome::Map(s1, s2)));
                    }
                    Poll::Ready(Ok(State::Consume(fut))) => {
                        self.futures.extend(futures);
                        return Poll::Ready(Ok::<_, crate::Error>(PenetrateOutcome::Customize(
                            fut,
                        )));
                    }
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
        }

        log::debug!("{} futures remaining", self.futures.len());

        Poll::Pending
    }
}

impl<SF, CF, A, S> Factory<(ServerFactory<SF, CF>, S)> for PenetrateFactory<S>
where
    SF: Factory<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    CF: Factory<Socket, Output = BoxedFuture<S>> + Send + Sync + 'static,
    A: Accepter<Stream = S> + Send + Unpin + 'static,
    S: Stream + Sync + Send + 'static,
{
    type Output = BoxedFuture<PenetrateGenerator<S, A>>;

    fn call(&self, (factory, mut client): (ServerFactory<SF, CF>, S)) -> Self::Output {
        let peer_factory = self.unpacker.clone();
        let config = self.config.clone();

        Box::pin(async move {
            let message = client.recv_packet().await?.try_message()?;

            let (socket, accepter) = match message {
                Message::Bind(Bind::Bind(addr)) => {
                    log::debug!("try to bind the server to {}", addr);
                    (addr.clone(), factory.bind(addr).await)
                }
                message => {
                    log::debug!("received an invalid message {}", message);
                    return Err(Kind::Unexpected(format!("{}", message)).into());
                }
            };

            match accepter {
                Err(e) => {
                    let message =
                        Message::Bind(Bind::Failed(socket, e.to_string())).to_packet_vec();

                    log::warn!("failed to create listener err={}", e);

                    if let Err(e) = client.send_packet(&message).await {
                        log::warn!("failed to send failure message to client err={}", e);
                    }

                    return Err(e);
                }
                Ok(accepter) => {
                    let message = Message::Bind(Bind::Bind(socket.clone())).to_packet_vec();
                    if let Err(e) = client.send_packet(&message).await {
                        drop(accepter);
                        log::warn!("failed to send message to client err={}", e);
                        Err(e)
                    } else {
                        log::info!(
                            "the listener was created successfully and bound to {}",
                            socket
                        );

                        Ok(PenetrateGenerator(Penetrate::new(
                            config,
                            peer_factory,
                            client,
                            accepter,
                        )))
                    }
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
        match ready!(Pin::new(&mut self.0).poll_accept(cx)?) {
            PenetrateOutcome::Customize(fut) => {
                log::debug!("custom mode");
                Poll::Ready(Ok(Some(fut)))
            }
            PenetrateOutcome::Map(s1, s2) => Poll::Ready(Ok(Some(Box::pin(async move {
                log::debug!("start forwarding");
                if let Err(e) = io::forward(s1, s2).await {
                    log::warn!("forward error {}", e);
                };
                Ok(())
            })))),
        }
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
