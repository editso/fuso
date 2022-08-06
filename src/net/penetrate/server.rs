use std::{collections::HashMap, fmt::Display, pin::Pin, sync::Arc, task::Poll, time::Duration};

use crate::penetrate::accepter::PenetrateAccepter;
use crate::penetrate::client;
use crate::protocol::IntoPacket;
use crate::sync::Mutex;
use std::future::Future;

use crate::io::{ReadHalf, WriteHalf};

use crate::{
    ext::AsyncWriteExt,
    generator::Generator,
    guard::Fallback,
    io,
    protocol::{AsyncRecvPacket, AsyncSendPacket, Bind, Poto, ToBytes, TryToPoto},
    ready, Accepter, Provider, Socket, Stream, WrappedProvider,
};

use super::accepter::Pen;
use super::mock::Mock;
use super::PenetrateObserver;
use crate::{join, time, Address, Error, Kind, NetSocket, Processor, Platform};

type BoxedFuture<T> = Pin<Box<dyn std::future::Future<Output = crate::Result<T>> + Send + 'static>>;

macro_rules! throw_client_error {
    ($result: expr) => {
        match $result {
            Ok(t) => t,
            Err(e) => {
                log::warn!("client error {}", e);
                return Ok(State::Error(e));
            }
        }
    };
}

macro_rules! read_client_config {
    ($client: expr) => {{
        let config = $client.recv_packet().await;

        if let Err(e) = config.as_ref() {
            log::warn!("client error {}", e);
            return Err(unsafe { config.unwrap_err_unchecked() });
        }

        let config = unsafe { config.unwrap_unchecked() };

        let config = bincode::deserialize::<client::Config>(&config.payload);

        if let Err(e) = config.as_ref() {
            log::warn!("configuration error {}", e);
            let err_msg = e.to_string().into_packet();
            let err_msg = err_msg.encode();
            if let Err(e) = $client.send_packet(&err_msg).await {
                log::warn!("client error {}", e);
            }
            return Err(e.to_string().into());
        } else {
            log::debug!("recv client config ");
            let ok_msg = "YES".into_packet();
            let ok_msg = ok_msg.encode();
            if let Err(e) = $client.send_packet(&ok_msg).await {
                log::warn!("client error {}", e);
                return Err(e);
            }
        }

        unsafe { config.unwrap_unchecked() }
    }};
}

pub enum Outcome<T> {
    Route(T, T),
    Future(BoxedFuture<()>),
}

pub enum State<T> {
    Stop,
    Close(T),
    Finish,
    Route(T, T),
    Provider(BoxedFuture<()>),
    Error(crate::Error),
}

pub enum Visitor<T> {
    Route(T),
    Provider(WrappedProvider<T, ()>),
}

pub struct PenetrateGenerator<P, T, A, O>(Penetrate<P, T, PenetrateAccepter<A, A>, O>);

pub enum Peer<T> {
    Route(Visitor<T>, Socket),
    Finished(T),
    Unknown(T),
}

#[derive(Default, Clone)]
pub struct MQueue<T> {
    identify: Arc<Mutex<u32>>,
    wait_list: Arc<async_mutex::Mutex<HashMap<u32, T>>>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub(super) whoami: String,
    pub(super) is_mixed: bool,
    pub(super) maximum_wait: Duration,
    pub(super) heartbeat_delay: Duration,
    pub(super) read_timeout: Option<Duration>,
    pub(super) write_timeout: Option<Duration>,
    pub(super) fallback_strict_mode: bool,
    pub(super) enable_socks: bool,
    pub(super) enable_socks_udp: bool,
    pub(super) socks5_password: Option<String>,
    pub(super) socks5_username: Option<String>,
    pub(super) platform: Platform
}

pub struct PenetrateProvider<T> {
    pub(crate) mock: Arc<Mock<T>>,
    pub(crate) config: Config,
}

pub struct Penetrate<P, S, A, O> {
    mock: Arc<Mock<S>>,
    config: Arc<Config>,
    accepter: A,
    address: Address,
    writer: WriteHalf<S>,
    processor: Processor<P, S, O>,
    futures: Vec<BoxedFuture<State<S>>>,
    mqueue: MQueue<async_channel::Sender<S>>,
    client_addr: Address,
}

impl<T> MQueue<T> {
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

impl Config {
    fn update(&mut self, config: client::Config) {
        self.whoami = config.name;
        self.enable_socks = config.enable_socks5 || config.enable_socks5_udp;
        self.enable_socks_udp = config.enable_socks5_udp;
        self.socks5_username = config.socks_username;
        self.socks5_password = config.socks_password;
        self.heartbeat_delay = config.heartbeat_delay;
        self.maximum_wait = config.maximum_wait;
        self.is_mixed = config.enable_kcp;
        self.platform = config.platform;
    }
}

impl<P, T, A, O> Penetrate<P, T, A, O>
where
    T: Stream + Sync + Send + 'static,
    A: Accepter<Stream = Pen<T>> + Unpin + Send + 'static,
    O: PenetrateObserver + Sync + Send + 'static,
    P: Sync + Send + 'static,
{
    pub fn new(
        config: Config,
        converter: Arc<Mock<T>>,
        processor: Processor<P, T, O>,
        address: Address,
        client: T,
        accepter: A,
    ) -> Self {
        let client_addr = unsafe { client.peer_addr().unwrap_unchecked() };
        let (reader, writer) = crate::io::split(client);

        let mqueue = MQueue {
            identify: Default::default(),
            wait_list: Default::default(),
        };

        let recv_fut = Self::poll_handle_recv(mqueue.clone(), reader.clone());
        let write_fut = Self::poll_heartbeat_future(writer.clone(), config.heartbeat_delay);

        Self {
            writer,
            config: Arc::new(config),
            mock: converter,
            accepter,
            mqueue,
            client_addr,
            processor,
            address,
            futures: vec![Box::pin(recv_fut), Box::pin(write_fut)],
        }
    }

    async fn poll_handle_recv(
        mqueue: MQueue<async_channel::Sender<T>>,
        mut stream: ReadHalf<T>,
    ) -> crate::Result<State<T>> {
        loop {
            let packet = stream.recv_packet().await;

            if packet.is_err() {
                let err = unsafe { packet.unwrap_err_unchecked() };
                log::warn!("client error {}", err);
                return Ok(State::Error(err));
            }

            let packet = unsafe { packet.unwrap_unchecked() }.try_poto();

            if packet.is_err() {
                log::warn!("The client sent an invalid packet");
                return Ok(State::Error(unsafe { packet.unwrap_err_unchecked() }));
            }

            let message = unsafe { packet.unwrap_unchecked() };

            match message {
                Poto::Ping => {
                    log::trace!("client ping received");
                }
                Poto::MapError(id, err) => {
                    log::warn!("client mapping failed, msg = {}", err);
                    mqueue.remove(id).await.map(|r| r.close());
                }
                message => {
                    log::warn!("ignore client message {:?}", message);
                }
            }
        }
    }

    async fn poll_heartbeat_future(
        mut stream: WriteHalf<T>,
        timeout: Duration,
    ) -> crate::Result<State<T>> {
        let ping = Poto::Ping.bytes();

        loop {
            log::trace!("send heartbeat packet to client");

            if let Err(e) = stream.send_packet(&ping).await {
                log::warn!("failed to send heartbeat packet to client");
                break Ok(State::Error(e));
            }

            time::sleep(timeout).await;
        }
    }

    fn async_penetrate_handle(self: &mut Pin<&mut Self>, pen: Pen<T>) -> BoxedFuture<State<T>> {
        let mut writer = self.writer.clone();
        let mock = self.mock.clone();
        let timeout = self.config.maximum_wait;
        let mqueue = self.mqueue.clone();
        let fallback_strict_mode = self.config.fallback_strict_mode;
        let processor = self.processor.clone();
        let config = self.config.clone();

        let fut = async move {
            match pen {
                Pen::Visit(visitor) => {
                    let mut fallback = Fallback::new(visitor, fallback_strict_mode);
                    let visit_addr = fallback.peer_addr()?;
                    let _ = fallback.mark().await?;
                    let peer = mock.call((fallback, config)).await?;
                    let (accept_tx, accept_ax) = async_channel::bounded(1);
                    let id = mqueue.push(accept_tx).await;

                    let (visitor, dst) = match peer {
                        Peer::Finished(visitor) => return Ok(State::Close(visitor.into_inner())),
                        Peer::Unknown(visitor) => return Ok(State::Close(visitor.into_inner())),
                        Peer::Route(visitor, dst) => (visitor, dst),
                    };

                    let route = Poto::Map(id, dst).bytes();

                    throw_client_error!(writer.send_packet(&route).await);

                    log::trace!("client notified, waiting for mapping");

                    match visitor {
                        Visitor::Route(src) => {
                            let mut src = src;
                            let mut dst = accept_ax.recv().await?;

                            src.backward().await?;

                            if let Some(data) = src.back_data() {
                                log::debug!("copy data to peer {}bytes", data.len());

                                if let Err(e) = dst.write_all(&data).await {
                                    log::warn!(
                                        "mapping failed, the client has closed the connection"
                                    );
                                    return Err(e.into());
                                }
                            }

                            processor.observer().on_pen_route(
                                &writer.peer_addr()?,
                                &visit_addr,
                                &dst.peer_addr()?,
                            );

                            Ok::<_, crate::Error>(State::Route(src.into_inner(), dst))
                        }
                        Visitor::Provider(provider) => {
                            let fallback =
                                Fallback::new(accept_ax.recv().await?, fallback_strict_mode);

                            processor.observer().on_pen_route(
                                &writer.peer_addr()?,
                                &visit_addr,
                                &fallback.peer_addr()?,
                            );

                            let dst = provider.call(fallback);

                            Ok(State::Provider(dst))
                        }
                    }
                }
                Pen::Client(client) => {
                    let mut client = processor.decorate(client).await?;

                    let poto = client.recv_packet().await?.try_poto()?;

                    match poto {
                        Poto::Map(id, _) => {
                            if let Some(tx) = mqueue.remove(id).await {
                                if let Err(_) = tx.send(client).await {
                                    log::warn!("the client established a mapping request, but the peer was closed");
                                }
                            }
                        }
                        poto => {
                            log::warn!("bad message {}", poto)
                        }
                    }

                    Ok(State::Finish)
                }
            }
        };

        let wait_fut = crate::time::wait_for(timeout, fut);

        Box::pin(async move {
            match wait_fut.await {
                Ok(ready) => ready,
                Err(e) => Err(e.into()),
            }
        })
    }
}

impl<P, T, A, O> NetSocket for Penetrate<P, T, A, O>
where
    T: Stream,
    A: Accepter<Stream = Pen<T>>,
{
    fn peer_addr(&self) -> crate::Result<Address> {
        self.accepter.peer_addr()
    }

    fn local_addr(&self) -> crate::Result<Address> {
        self.accepter.local_addr()
    }
}

impl<P, T, A, O> Accepter for Penetrate<P, T, A, O>
where
    T: Stream + Send + Sync + 'static,
    A: Accepter<Stream = Pen<T>> + Unpin + Send + 'static,
    O: PenetrateObserver + Sync + Send + 'static,
    P: Send + Sync + 'static,
{
    type Stream = Outcome<T>;

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
                Poll::Ready(pen) => {
                    futures.push(self.async_penetrate_handle(pen));
                    true
                }
            };

            while let Some(mut future) = futures.pop() {
                match Pin::new(&mut future).poll(cx) {
                    Poll::Pending => {
                        self.futures.push(future);
                    }
                    Poll::Ready(Ok(State::Route(s1, s2))) => {
                        self.futures.extend(futures);

                        return Poll::Ready(Ok::<_, crate::Error>(Outcome::Route(s1, s2)));
                    }
                    Poll::Ready(Ok(State::Provider(fut))) => {
                        self.futures.extend(futures);
                        return Poll::Ready(Ok::<_, crate::Error>(Outcome::Future(fut)));
                    }
                    Poll::Ready(Ok(State::Stop)) => {
                        log::warn!("client aborted {}", self.client_addr);
                        return Poll::Ready(Err(crate::error::Kind::Channel.into()));
                    }
                    Poll::Ready(Ok(State::Error(e))) => {
                        log::warn!("client error {}, err: {}", self.client_addr, e);
                        self.processor.observer().on_pen_error(&self.address, &e);
                        return Poll::Ready(Err(e));
                    }
                    Poll::Ready(Ok(State::Close(mut s))) => {
                        futures.push(Box::pin(async move {
                            let _ = s.close().await;
                            Ok(State::Finish)
                        }));
                    }
                    _ => {}
                }
            }
        }

        log::debug!("{} futures remaining", self.futures.len());

        Poll::Pending
    }
}

impl<P, A, S, O> Provider<(S, Processor<P, S, O>)> for PenetrateProvider<S>
where
    A: Accepter<Stream = S> + Send + Unpin + 'static,
    S: Stream + Sync + Send + 'static,
    P: Provider<Socket, Output = BoxedFuture<A>> + Send + Sync + 'static,
    O: PenetrateObserver + Send + Sync + 'static,
{
    type Output = BoxedFuture<PenetrateGenerator<P, S, A, O>>;

    fn call(&self, (mut client, processor): (S, Processor<P, S, O>)) -> Self::Output {
        let peer_provider = self.mock.clone();
        let mut config = self.config.clone();
        Box::pin(async move {
            let poto = client.recv_packet().await?.try_poto()?;
            let penetrate = match poto {
                Poto::Bind(Bind::Setup(client_addr, visit_addr)) => {
                    log::debug!("try to bind the server to {}", visit_addr);
                    let visit_fut = processor.bind(visit_addr);
                    let client_fut = processor.bind(client_addr);
                    join::join_output(client_fut, visit_fut).await
                }
                message => {
                    log::debug!("received an invalid message {}", message);

                    let err: Error = Kind::Unexpected(format!("{}", message)).into();

                    processor
                        .observer()
                        .on_pen_error(&client.peer_addr()?, &err);

                    return Err(err);
                }
            };

            match penetrate {
                Err(e) => {
                    let message = Poto::Bind(Bind::Failed(e.to_string())).bytes();

                    log::warn!("failed to create listener err={}", e);

                    if let Err(e) = client.send_packet(&message).await {
                        log::warn!("failed to send failure message to client err={}", e);
                        processor.observer().on_pen_error(&client.peer_addr()?, &e);
                    }

                    Err(e)
                }
                Ok((aclient, avisit)) => {
                    let visit_addr = avisit.local_addr()?;
                    let client_addr = aclient.local_addr()?;

                    let poto = Poto::Bind(Bind::Success(client_addr, visit_addr));

                    client.send_packet(&poto.bytes()).await.map_err(|e| {
                        log::warn!("failed to send message to client err={}", e);
                        e
                    })?;

                    let client_config = read_client_config!(client);

                    config.update(client_config);

                    processor.observer().on_pen_start(
                        &client.peer_addr()?,
                        &avisit.local_addr()?,
                        &aclient.local_addr()?,
                        &config,
                    );

                    log::info!(
                        "client is {} and the server is {}",
                        client.peer_addr()?,
                        aclient.local_addr()?
                    );

                    log::info!("please visit {} for port mapping", avisit.local_addr()?);

                    Ok(PenetrateGenerator(Penetrate::new(
                        config,
                        peer_provider,
                        processor,
                        client.peer_addr()?,
                        client,
                        PenetrateAccepter::new(avisit, aclient),
                    )))
                }
            }
        })
    }
}

impl<P, T, A, O> Generator for PenetrateGenerator<P, T, A, O>
where
    A: Accepter<Stream = T> + Send + Unpin + 'static,
    T: Stream + Send + Sync + 'static,
    O: PenetrateObserver + Sync + Send + 'static,
    P: Send + Sync + 'static,
{
    type Output = Option<BoxedFuture<()>>;

    fn poll_generate(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> Poll<crate::Result<Self::Output>> {
        match ready!(Pin::new(&mut self.0).poll_accept(cx)?) {
            Outcome::Future(fut) => {
                log::debug!("start a future");
                Poll::Ready(Ok(Some(fut)))
            }
            Outcome::Route(s1, s2) => Poll::Ready(Ok(Some(Box::pin(async move {
                log::debug!("start forwarding");
                if let Err(e) = io::forward(s1, s2).await {
                    log::trace!("forward error {}", e);
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
            self.read_timeout, self.write_timeout, self.maximum_wait, self.heartbeat_delay
        )
    }
}
